---
status: in-progress
type: feature
priority: high
area: backend
---

# Auth flows: bootstrap, registration, and user management

## Problem

A fresh Picasu install has no supported way to enroll its first user
through the product itself. The backend accepts an unauthenticated
`POST /post/users/create` while the store is empty (forced admin) and
migrates a pre-seeded legacy config password on first login, but no
frontend code calls `users/create` (`UserManagement.vue` only lists
users and toggles the admin flag), so a web-only deployer must curl
the endpoint or edit `config.toml` on the host. Until someone enrolls,
an unset-password install is fully admin-open to the reachable network
(F6 in `.plan/auth-hardening.md`).

Beyond the first user there is no enrollment at all: no
self-registration, no admin-create UI, no invitation mechanism — and
no flow-by-flow review of what exists (login, migration, password
change, promote/demote) to pin what works. This task iterates over
all user-based flows to get them working.

## Strategy

Online enrollment becomes opt-in; host-side provisioning covers the
rest:

1. **Host-based initial password stays as-is.** A pre-seeded legacy
   `password` in `config.toml` keeps working: guards deny, first login
   migrates the claimed id to first admin. No code change.
2. **`--bootstrap-passwd` enables bootstrap of the first user.**
   New server flag (also `PICASU_BOOTSTRAP_PASSWD` env and a config-file
   key): while it is set **and the user store is empty**, the current
   open first-run behavior applies (ephemeral admin identity,
   accept-anything `authenticate`, unauthenticated `users/create`
   forced-admin). Without it, a no-admin install answers 401
   everywhere — closed by default, which also resolves the F6
   exposure for fresh installs. The gate is "empty store", not
   "no admin user": after the first user exists, adminship is never
   acquired online except by an existing admin granting it — an
   admin-less non-empty store stays closed online and recovers via
   the CLI (kills the silent-reopen class: a demotion can never
   re-open enrollment). Legacy-password installs keep the
   deny-and-migrate flow regardless of the flag (an explicit host
   secret wins), except unauthenticated `users/create`, which is
   denied in legacy mode too — otherwise the secret would not
   actually gate enrollment (security review A1).
3. **Fail fast on a non-fresh store.** With the flag set, the server
   refuses to launch when `<DATA_HOME>/auth/passwd.json` already
   exists: either bootstrap already happened (remove the flag) or the
   store needs manual inspect/repair (via `picasu-admin`). A pure
   file-existence check at boot, before serving — the safest failure
   mode, and it turns a forgotten flag on a healthy install into a
   loud error instead of silent intent mismatch.
4. **Registration screen when enrollment is open.** `/login` probes
   `GET /get/users` on mount: a list with no admin entry means enroll
   mode (register → auto-login); 401 means login mode (legacy password
   or closed install — the server's error text says which). No new
   route: the probe reuses the documented admin endpoint.
5. **Separate CLI binary for host-side provisioning and recovery.**
   New `picasu-admin` bin target reusing `initialize()` and the
   `auth::users` / `auth::password` stores directly (independent file
   access, no server protocol): `add-user <id> [--admin]`,
   `set-password <id>`, `set-admin <id> on|off`. Password from
   `--password`, else hidden prompt (`rpassword` — zero-dep,
   MIT/Apache; confirm `cargo deny` stays green), else stdin pipe.
   Operational rule: server stopped (or restarted after) for changes;
   concurrent server writes are the admin's problem.
6. **Self-registration, gated by the admin (backend).** New
   `allow_registration` knob (config key + `PICASU_ALLOW_REGISTRATION`
   env + runtime toggle via the `PUT /put/config` allowlist, default
   off) plus single-use invitation tokens:
   - `POST /post/users/register {userId, password, invite?}` takes
     no auth guard (public, like `authenticate`) plus the read-only
     guard. With the knob on, anyone registers; otherwise a valid
     invite is required. Forced `admin: false` always — bootstrap
     `users/create` stays the only online admin-granting path, and
     only on an empty store.
   - Invites live in a new redb `INVITES` table (same `USERS`
     `TableDefinition<&str, &str>` + JSON pattern): random 128-bit
     token, only the blake3 hash stored (compare with `subtle`),
     configurable expiry in hours (required `expiresInHours`, `0`
     never expires — 168 is the conventional 7-day lifetime) resolved
     by `auth::invites::resolve_expiry`, creator/expiry/used-by
     metadata plus a short prefix hint for admin display. Redeem
     marks used in the same redb transaction as the user insert
     (passwd.json stays file-atomic outside it; either crash order
     fails safe — no login without both halves).
   - `POST /post/invites/create` (admin + read-only guard) returns
     the token once; `GET /get/invites` (admin) lists metadata, never
     tokens; revoke follows the existing delete-route conventions.
7. **Registration UI (frontend).** `RegisterPage.vue` + `/register`
   route (`?invite=` prefilled for the bearer flow, open flow
   otherwise; password-confirm; success auto-logs-in via the
   existing `login()` + cookie path); `LoginPage` links to it.
   Admin panel extends `UserManagement.vue` (or a sibling section):
   registration toggle, invite create (link shown once, copy
   button — client builds
   `${origin}/register?invite=${token}`), invite list + revoke, and
   the missing admin-create-user form (`users/create` with the admin
   cookie). `createUser`/`registerUser`/invite calls in
   `frontend/src/api/users.ts` (+ vitest); `autocomplete` +
   confirm fields per the privacy note.

Note on scope: non-admin users today pass only `GuardUser` routes
(password self-service and little else) — registration's value is
the invite primitive and forward-looking per-user grants, not
current capabilities. Per-user album grants stay out of scope.

Rejected: a one-time setup token (extra endpoint, UI state and
invalidation bookkeeping — obsolete once closed-by-default lands); a
new `GET /get/auth/status` endpoint (the `users` probe already
distinguishes the states: admin-less list vs 401); config-password as
the only path (requires host access on every install — poor Docker
UX); CLI as a `picasu` subcommand (a separate binary keeps the server
binary's argument surface untouched and matches "small cli tool");
multi-use invites and email verification (no mail infra in
self-hosted scope — the admin distributes links out of band).

Verified constraints (from sources, not memory):

- redb 4.x forbids a second process opening a read-write `Database`
  (`~/.cargo/registry/.../redb-4.2.0/src/db.rs`: only
  `ReadOnlyDatabase` may be shared; multiprocess writes need the
  `experimental-multiprocess` feature, which is not enabled). The CLI
  therefore targets a stopped server and takes an exclusive lock on a
  lockfile under `<DATA_HOME>` before touching either store; the
  server holds the same lock shared for its lifetime, so a CLI run
  against a live server fails fast with "stop the server first"
  instead of corrupting the database. Locking needs one tiny dep
  (`fs2` — zero-dep, MIT/Apache; confirm `cargo deny` stays green);
  where the filesystem cannot lock, both sides warn and continue, and
  concurrent use stays the admin's problem.
- `mod init` is private in `backend/src/lib.rs`, so the CLI needs a
  re-export of `initialize`/`initialize_logger` (precedent: the
  existing test re-exports next to it). `auth` is already `pub`.
- No `fs2`/`rpassword` in the tree today; both are leaf crates, added
  to `[dependencies]` with a `deny.toml`-compatible license (verify
  with `just backend-audit`).

## Steps

1. **Enrollment gate (backend).** Add `bootstrap_passwd: bool`
   (default false) to config with `PICASU_BOOTSTRAP_PASSWD` env
   override and `--bootstrap-passwd` server flag; gate the "store
   empty" open-mode conditions in `try_jwt_cookie_auth`, `GuardUser`,
   and `authenticate` on the flag, and narrow `create_user_handler`'s
   bootstrap exception to empty-store-only while denying
   unauthenticated create in legacy mode (review A1; legacy migration
   paths unchanged — no `has_admin()` helper needed under the
   simplified invariant). Closed-mode rejections say "no
   administrator enrolled". Boot: pure `bootstrap_freshness_ok(flag,
passwd_exists)` predicate — flag + existing `passwd.json` refuses
   to launch with an actionable message (remove the flag, or
   inspect/repair via CLI); unit-tested. WARN log at boot whenever
   the flag is on; log every enrollment event (create, migration).
   Update the test harness (`tests/bootstrap.rs`) to opt into
   bootstrap mode where tests rely on open behavior, set
   `PICASU_BOOTSTRAP_PASSWD=true` in the Playwright launcher env
   (e2e seeds its password via open-mode routes), and add gate tests
   (closed by default, open with flag on empty store, closes once
   any user exists, admin-less non-empty store stays closed with CLI
   recovery, legacy ignores the flag, freshness refusal).
   `just run` passes `--bootstrap-passwd` so the sandbox stays
   demo-able.
   Leaves out: the concurrent-first-create race (unverified; verify
   with a test here, fix — share the lock — only if confirmed).
2. **CLI binary + lock.** New `[[bin]] picasu-admin`
   (`src/bin/picasu-admin.rs`): pure `parse` fn with unit tests
   (`add-user`, `set-password`, `set-admin`); `main` runs
   `initialize()`, takes the exclusive store lock (fail fast naming a
   running server), dispatches onto the shared provision helper from
   step 3. Server side: acquire the shared lock in `initialize()`.
   Dockerfile builds and ships the second binary.
   Leaves out: `list-user`/`delete-user` (list exists via UI; delete
   has no route either).
3. **Shared provisioning helper (backend).** Extract `create_user_sync`
   from `backend/src/router/post/users.rs` into
   `auth::users::provision_user(id, password, admin)` (validate id,
   trim, default password policy for new passwords —
   `auth::password::check_strength`: 8–256 chars, shipped blocklist,
   user-id and trivial-pattern rules; `--trivial-passwords` bypass —
   duplicate error, write redb record + passwd file); route and CLI
   both delegate. Unit tests: duplicate, weak/empty password,
   trim-then-login consistency. Done 2026-10-13 (commits below).
4. **Registration screen (frontend).** `createUser` in
   `frontend/src/api/users.ts` (+ vitest); `LoginPage.vue` bootstrap mode
   from the `listUsers` probe (list with no admin → register form with
   password-confirm, create → `login()` → cookie → redirect;
   create-409 → fall back to login with a notice). Component/API-level
   vitest.
   Leaves out: E2E scenario (probe + create + login are covered at API
   level; revisit if flaky).
5. **Registration backend (spike — worker, then senior review).**
   Scope: `auth::users::provision_user` extraction (route delegates,
   behavior preserved) + default password policy for new passwords
   (provision and `set_password_sync`: 8–256 chars, shipped
   blocklist, id/pattern rules; `--trivial-passwords` bypass); `allow_registration` knob (config key +
   env + `PUT /put/config` allowlist); `INVITES` table +
   `auth::invites` helpers (create/list/redeem, blake3-hashed
   tokens, `subtle` compare, 7d default expiry, prefix hint);
   routes `POST /post/users/register` (no auth guard + read-only
   guard, forced `admin: false`), `POST /post/invites/create`,
   `GET /get/invites` (both `GuardAuth`), revoke per delete-route
   conventions; bootstrap-create narrowing (empty-store-only +
   legacy-deny) in `create_user_handler`; full `#[utoipa::path]`
   annotations. Done when: route matrix in
   `backend/src/tests/register.rs` is green (open/invite/disabled/
   expired/double-use/revoked/duplicate/short-password/legacy-deny),
   `cargo test` builds the whole backend suite, `just
backend-check` and `just openapi-check` (after `openapi-gen`)
   pass. Must-reads before code: `docs/briefs/rocket-guards.md`,
   `post/users.rs`, `put/users_admin.rs`, `storage/db.rs` table
   pattern. Out: bootstrap flag/guards/freshness/harness (owner
   follows), CLI, frontend, docs, commits (report only).
6. **Registration frontend.** `RegisterPage` + `/register`,
   `LoginPage` link, admin-panel section (toggle, invite
   create/list/revoke, admin-create form), api fns + vitest. Starts
   after the spike's API shape is reviewed.
7. **Docs + gates.** `docs/auth.md` (enrollment paths, flag,
   freshness rule, registration + invites, CLI rule, lockfile),
   `docs/config.md` keys + envs, `docs/linux.md` if it shows
   first-run, README env-var rows. New routes and the
   `bootstrapPasswd`/`allowRegistration` schema keys mean regenerating
   `backend/openapi.json` (`just openapi-gen`) and keeping
   `just openapi-check` green. Gates: `backend-check`,
   `frontend-check`, `cargo test`, frontend vitest, `docs-check`,
   `plan-lint`, `backend-audit` (new deps).

## Decisions (settled 2026-10-13, security review same day)

- Bootstrap-mode detection: `GET /get/users` probe, no new endpoint.
- CLI password input: hidden prompt via `rpassword` + `--password`
  escape hatch (with stderr warning) + stdin pipe, in that priority
  (verify `cargo deny` green).
- CLI scope: `add-user`, `set-password`, `set-admin` (recovery needs
  all three).
- CLI shape: separate `picasu-admin` binary with direct store access
  (not a subcommand); cooperative file lock held (not checked),
  server holds shared for life; CLI fail-closes when locking is
  unsupported, server warn-and-continues.
- Online enrollment gated on "no admin user exists", enabled only by
  `--bootstrap-passwd` / env / config; closed by default.
- Freshness: flag + existing `passwd.json` refuses to launch
  (actionable message); a forgotten flag fails loud, never silent.
- Legacy mode denies unauthenticated `users/create` (review A1) —
  the host secret actually gates enrollment; migration paths only.
- New-password policy (review A3, settled 2026-10-13): NIST
  800-63B-shaped — 8–256 chars, shipped blocklist (SecLists 500 +
  service appendix, case-insensitive), user-id equality and
  trivial-pattern rejection, no composition rules or expiry; PBKDF2
  work factor plus planned online throttling carry the guessing
  resistance. Enforced in the shared provision and set-password
  paths only (no retroactive enforcement on legacy migration).
  `--trivial-passwords` (flag/env/config, default off,
  warn-logged) bypasses strength but never presence (empty stays
  400).
- Closed-vs-legacy 401 oracle accepted deliberately: the distinct
  "no administrator enrolled" message aids deployer UX and reduces
  attacker incentive (review A5).
- Guest-only mode removed 2026-10-13: needs the F9
  `import_config` fix, a read-route audit, and a frontend mode
  signal — re-propose as its own task if wanted.
- Spike model: `opencode/mimo-v2.6-flash-free` (2.5 not offered by
  the provider); worker reports, senior reviews before commit.
- Spike review 2026-10-13: accepted as committed. Verified
  `backend-check`, targeted suites (register/invite/auth/config),
  and the pre-existing partial-filter failure (`cargo test --lib
user`: `reset_backend_state_clears_user_store` order-dependence,
  reproduced on pristine HEAD — full suite is the gate).
  Follow-ups for step 6: `ConfigResponse` (GET /get/config) does not
  expose `allowRegistration` yet — the admin-panel toggle needs it;
  register/invite failures use 403/404/409 (never 401), so the
  register UI must not route them through the login-redirect
  interceptor path.
