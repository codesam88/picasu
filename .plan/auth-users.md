---
status: in-progress
type: feature
priority: high
area: backend
---

## Notes

Multi-user authentication. PBKDF2 password store in a separate file;
user-id-indexed role records in redb; admin as a role property of users;
guest (anonymous/share) access preserved. Album access and per-user settings
values are out of scope; any new knobs are admin-level (global) settings.

### Bounded change

- Outcome: multiple users can be created (first via bootstrap), log in by
  user id + password, hold the admin role or not; admin routes require the
  role; guest/share behavior unchanged in shape.
- In scope: new `backend/src/auth/` module (PBKDF2 store, user records);
  `Claims`/`Role` identity shape; `try_jwt_cookie_auth` role liveness;
  `authenticate` user login + legacy-string bootstrap; user management
  endpoints (create, set password, list); legacy single-password migration;
  `update_password_handler` rework onto the store; fixtures, tests,
  `openapi.json`, `docs/auth.md` + `docs/authz-model.md` reconcile.
- Out: album ACL/grants, per-user settings values, user delete/demote
  endpoints (helpers support it; no route), migration UI, rate limiting,
  cookie/HttpOnly changes.
- Checks: `cargo test`, `just backend-check`, `just openapi-check`,
  `docs-check`, `plan-lint`.

### Decisions

- KDF: PBKDF2-HMAC-SHA256, per-user 16-byte salt (`rand`), 600k iterations,
  constant-time verify; JSON file at `<DATA_HOME>/auth/passwd.json`
  (`{"users": {id: {salt, hash, iterations}}}`), atomic write (tmp+rename),
  best-effort `0600` on unix. New deps: `pbkdf2`, `sha2` (`rand`, `base64`
  already present).
- User record in redb (`users` table, JSON string value like `ASSET_BY_ID`):
  `{ admin: bool }` keyed by user id. Extensible; no per-user settings yet.
- Identity claims: `Role::User { id, admin }` replaces `Role::Admin`;
  `Role::Share` unchanged. Old tokens fail decode (same effect as key
  rotation, as with the `typ` migration).
- `GuardAuth` = admin role required; role re-read from the DB per request so
  demotion takes effect before token expiry. `GuardShare` admin fallback
  generalizes to any admin-role user.
- Empty user store = open first-run mode (as today with unset password).
- `POST /post/authenticate` accepts `{ userId, password }`; a bare JSON
  string body remains as the legacy bootstrap path, valid only while the
  store is empty (verifies the legacy config password, migrates it to user
  `admin`). Legacy `password` is ignored once the store is non-empty.
- User management is admin-only, except unauthenticated creation of the
  first user (forced `admin: true`) while the store is empty.
- `PUT /put/config/password` keeps its route; new semantics: admin may set
  any user's password without the old one, non-admin only self with correct
  old password; empty new password is 400 (no more "clear to open" — open
  mode is store-empty only). Needs the caller's id, so `_auth: GuardAuth`
  becomes a consumed `auth: GuardAuth` (still dispatch-time denial).
- Password hashes are never exported; config export shape unchanged.

### Steps

Each substep is TDD: RED (failing test first, watched fail) → GREEN
(minimal code) → verify. Every step leaves the tree green.

#### S0 — Deps and module skeleton

- S0a: `cargo add pbkdf2 sha2 -p picasu`; `cargo build` green.
- S0b: `backend/src/auth/mod.rs` + `password.rs` skeleton with one ignored
  placeholder test removed in S1 (no placeholder tests kept).
- Verify: build passes, nothing else touched.

#### S1 — PBKDF2 store (pure, no HTTP)

- S1a RED: `hash_verify_roundtrip` — hash then verify accepts. Fails
  (no function). GREEN: `hash_password`/`verify_password`.
- S1b RED: wrong password rejected; tampered salt/iterations rejected;
  two hashes of the same password differ (salt uniqueness). GREEN.
- S1c RED: `PasswdFile` save/load round-trip on a tempdir; missing file =
  empty; corrupt file = error, never panic. GREEN: JSON file + atomic
  write + `0600` best-effort.
- S1d RED: `set_password` then `verify` through the file store; unknown
  user fails. GREEN: `PasswdFile::{set, verify, remove?}` (no remove —
  out of scope).
- Verify: `cargo test --lib auth::`, `backend-check`. Hashing runs on the
  calling thread here (pure functions); request paths must use
  `spawn_blocking` (S3/S4).

#### S2 — User records in redb

- S2a RED: `create_user` + `get_user` round-trip. GREEN: `USERS` table
  (`TableDefinition<&str, &str>`, JSON value), `UserRecord { admin: bool }`,
  helpers `create_user/get_user/set_admin/list_users/user_count`.
  Table auto-created on write paths; reads tolerate absence (empty).
- S2b: `reset_backend_state` clears the users table and removes the passwd
  file, so existing open-mode tests keep passing. RED first: test asserting
  empty store after reset.
- Verify: full `cargo test --lib` green (existing suites unaffected).

#### S3 — Claims identity + login path

- S3a RED: `Claims::new_user(id, admin)` encode→`decode_typed` round-trip
  preserves id/roles/`typ`; a legacy `Role::Admin` token is rejected.
  GREEN: `Role::User { id, admin }`, `is_admin`, `get_share` unchanged;
  update the (few) `new_admin`/`Role::Admin` callers.
- S3b RED: `try_jwt_cookie_auth` — valid admin-role token accepted; token
  for a demoted/removed user rejected (liveness); share-role token
  rejected; empty store + no legacy password = open. GREEN: role re-read
  from redb per request.
- S3c RED: `authenticate` with `{ userId, password }` mints a user-bound
  token on success, 401 on wrong/unknown. GREEN: handler rework
  (untagged `Legacy(String)` / `Login{userId,password}` body;
  `spawn_blocking` for hashing).
- S3d RED: legacy string body migrates (`store empty` + legacy config
  password → user `admin` created, token minted); legacy body rejected once
  the store is non-empty. GREEN: `ensure_migrated_from_legacy`.
- S3e: `auth_cookie` fixture posts the new login body (open mode accepts
  any). Existing suites must pass unmodified otherwise.
- Verify: `cargo test --lib`, `backend-check`.

#### S4 — Management endpoints + password-route rework

- S4a RED: `POST /post/users/create` — admin creates user + non-admin;
  duplicate → 409; non-admin → 401; unauthenticated with non-empty store → 401. GREEN.
- S4b RED: unauthenticated creation while store empty succeeds and forces
  `admin: true` (bootstrap). GREEN.
- S4c RED: `GET /get/users` lists `{userId, admin}` without hashes,
  admin-only. GREEN.
- S4d RED: `PUT /put/users/password` — admin sets any password without old;
  self-service with correct old password; wrong old → 401; cross-user by
  non-admin → 403; empty new → 400. GREEN.
- S4e: rework `PUT /put/config/password` onto the same logic; keep route +
  405 behavior. RED-first via S4d-style cases against the old route. As
  built, the old route takes `GuardUser` (any authenticated user, self
  only) rather than `GuardAuth`, since bare `GuardAuth` would deny the
  self-service case — the plan's letter was wrong, intent kept.
- New `GuardUser` guard (any authenticated user, share tokens rejected,
  open-mode ephemeral identity) introduced for the self-service routes;
  recorded in the authz-model guard table + openapi-sanity pins at S6.
- `set_password_sync` auto-bootstraps: unknown target with an empty store
  creates it as admin (password routes double as first-user bootstrap).
- Verify: endpoint tests green, `backend-check`.

#### S5 — Integration matrix (`tests/auth_users.rs`)

One test per row; each written to fail before its code (most arrive green
via S3/S4 — then each gets a falsification companion per authz-props S0f):

- bootstrap: empty store is open; first user forced admin.
- two users (admin + bob): login each; admin routes OK for admin, 401 for
  bob; unknown user / wrong password 401.
- guest: anonymous share prefetch + serving works; admin-role user passes
  share-guarded routes without share headers.
- password change + demotion-via-helper take effect (liveness).
- legacy migration end-to-end with `APP_CONFIG.password` set directly.
- existing suites (`authz`, `backend_api`, `api_*`) pass unmodified apart
  from the fixture change; `api_config`/`api_first_launch` string bodies
  exercise the legacy path.

#### S6 — Release the change

- `just openapi-gen`, `just openapi-check` (new 3 endpoints + new
  authenticate body + password-route semantics).
- Full `cargo test`, `just backend-check`, `docs-check`, `plan-lint`.
- Reconcile `docs/auth.md` (admin sign-in, token table) and
  `docs/authz-model.md` (identity row, AUTH-11 as role check); note old-token
  invalidation + legacy-password migration in the commit message.
- Mark plan `done`.

## Progress

- 2026-10-10: plan flushed out; starting S0.
