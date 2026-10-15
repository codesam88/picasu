---
status: in-progress
type: feature
priority: high
area: backend
---

# Auth enrollment: first-user bootstrap

## Problem

Fresh install has no explicit first-user enrollment. `just run` lands on
the album view with no login; settings password-PUT silently creates an
invisible `admin` user; logout then login-as-admin fails (unknown id).
Root cause (audit 2026-10-13): no registration UI calls `users/create`;
password-change routes double as silent enrollment; `authenticate`
open-mode mints tokens without persisting a user.

## Flows

### A. `bootstrap_admin=false` (default, closed)

- Empty or non-empty store: every guarded route 401 `no administrator
enrolled`. No probe list, no enroll, no login.
- Frontend: probe `GET /get/users` → 401 ⇒ locked screen ("restart with
  `--bootstrap-admin` or provision via CLI"). No album access.

### B. `bootstrap_admin=true`, empty store

- Only `POST /post/users/bootstrap {userId, password}` enrolls, forcing
  the first admin. No `admin` field exists on any request body.
- Frontend forces enrollment first: probe `[]` ⇒ full-screen admin
  registration (username, password, confirm). Submit → bootstrap →
  auto-`login()` → cookie → `/timeline`. 409 ⇒ another admin won the
  race: fall back to `/login` with notice. Any other probe result ⇒
  normal login. No album/config access before enrollment completes.

### C. After first user exists

- Window closes (store non-empty). `POST /post/users/register`
  creates standard users only, allowed with an admin session, or open
  registration, or a valid invite. Elevation is admin-only via
  `PUT /put/users/admin`. Admin-less non-empty stays closed (CLI).

## Reverts applied

- `just run`: bootstrap flag only (drop `ALLOW_REGISTRATION`,
  `TRIVIAL_PASSWORDS`, `PBKDF_ITERATIONS=100` demo conveniences).
- Keep: gate, freshness refusal, harness opt-in, launcher env,
  provision/policy/invite backend, matrices.

## Reverts pending (TDD, with test updates)

- Password-PUT auto-create (`set_password_sync` empty-store branch):
  remove silent enrollment; unknown target ⇒ 404 even on empty store.
  Update `users_password::empty_store_bootstraps_first_user_as_admin`.
- `authenticate` open-mint: keep route but document "creates nothing";
  UI never calls it during enrollment (probe + create + login only).

## Test cases (backend matrices)

`enroll_gate.rs` (bootstrap route; `users/create` gone):

1. Flag off, empty: `bootstrap`/`authenticate`/`get/users` → 401, count 0.
2. Flag on, empty: probe `[]` → bootstrap forces admin → second call
   401 → login 200.
3. Flag on, seeded non-admin: `bootstrap` 401, count unchanged.
4. Password-PUT empty store, no cookie: 404, no user made.
5. Freshness predicate + boot-gate on real `passwd.json`.
6. No `admin` field accepted anywhere (unknown-field shape → 4xx).

`register.rs` (merged route, always non-admin): 7. Admin session creates without invite → 200, `admin: false`. 8. Non-admin/anonymous + knob on → 200, invite ignored not burned. 9. Knob off + invite → 200, invite burned; without → 403, count 0. 10. Expired → 403; used/duplicate → 409; revoked/unknown → 404;
weak → 400.

## Test cases (frontend, vitest + component)

1. Probe `[]` ⇒ registration screen shown, album not rendered.
2. Probe 401 ⇒ locked screen, no registration form.
3. Register submit: mismatched passwords blocked client-side; 409 ⇒
   login redirect + notice; 200 ⇒ cookie + `/timeline`.
4. Register failures (400/403/404) never trigger login redirect.

## Steps (one worker item each, TDD red-first, no commits by worker)

1. Backend revert: password-PUT 404-on-unknown + log line; update
   matrix row 5. Gates: targeted `cargo test`, `backend-check`.
2. Frontend enroll: `createUser` api + probe routing + `RegisterPage`
   (username/password/confirm) + 409 fallback. Gates: vitest,
   `frontend-check`.
3. E2E probe test: fresh sandbox ⇒ registration screen before album;
   register ⇒ timeline. Playwright scenario only.
4. Docs: `docs/auth.md` bootstrap section, config key table, README
   row. `docs-check`, `plan-lint`.
5. Self-change requires current (backend + UI): `set_password_sync`
   verifies old whenever target == caller (admins included); admin →
   other keeps no-old recovery. Change form requires current
   client-side; settings modal header shows username + role
   read-only. Gates: `cargo test`, vitest, both checks.
6. Admin panel UI: admin-create-user form, set-password control,
   registration toggle, invite create (link shown once) / list /
   revoke; needs `ConfigResponse.allowRegistration` read-back (add
   the field backend-side in this step). Gates: vitest,
   `frontend-check`, `openapi-check` after regen.
7. CLI provisioning: separate `picasu-admin` binary (`add-user`,
   `set-password`, `set-admin`) with lockfile discipline vs a live
   server; Dockerfile ships it. Gates: `cargo test`, `backend-check`.
8. First-create race: prove with a test whether concurrent
   unauthenticated bootstraps both land; share the bootstrap lock only
   if confirmed, else close with the negative result.
9. Merge create+register, add `users/bootstrap` (backend first, then
   frontend): delete `POST /post/users/create`; new
   `POST /post/users/bootstrap {userId, password}` (flag ∧ empty only,
   forced admin, same freshness/error semantics as the old bootstrap
   arm); `POST /post/users/register` accepts an admin session as a
   third gate (no invite then) and never mints admin; no `admin`
   field on any body. Migrate matrices (rows above), e2e
   given-runner + scenario covers to the new endpoints, openapi regen
   - pins + dated oasdiff ignores. Then frontend: `RegisterPage`
     calls bootstrap; `createUser` targets register without the flag.
     Gates: `cargo test`, vitest, all checks.

## Decisions

- Start is always the admin registration screen (explicit username —
  never an invisible `admin`).
- Gate stays "flag ∧ empty store" (not has-admin); admin-less
  non-empty recovers via CLI only.
- No new status endpoint; probe is `GET /get/users`.
- Creation never grants admin (2026-10-15): no `admin` field on any
  request body. Bootstrap has its own route (`POST
/post/users/bootstrap`, flag ∧ empty only); `register` merges
  admin-session, open-knob, and invite-bearer gates, always
  non-admin; elevation is `PUT /put/users/admin` only.

## Review claims 2026-10-15 (triaged elsewhere)

Four parallel worker reviews reported R1–R35 on branch `auth-enrollment` @
`442162a0c`, triaged 2026-10-16 in `.plan/auth-enrollment-review.md`.
That file is authoritative (verdicts, severity x impact, tackle order);
verbatim claims remain in git history.
