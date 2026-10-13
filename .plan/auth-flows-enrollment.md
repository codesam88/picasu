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

- Empty or non-empty store, no legacy password: every guarded route 401
  `no administrator enrolled`. No probe list, no create, no login.
- Frontend: probe `GET /get/users` → 401 ⇒ locked screen ("restart with
  `--bootstrap-admin` or provision via CLI"). No album access.
- Legacy password set: unchanged deny-and-migrate via `authenticate`.

### B. `bootstrap_admin=true`, empty store, no legacy password

- Backend stays as built (flag ∧ empty opens `users/create`,
  `authenticate`, probe `GET /get/users` → `[]`).
- Frontend forces enrollment first: on boot probe `GET /get/users` →
  `200 []` ⇒ full-screen admin registration (username, password,
  password-confirm). Submit → `POST /post/users/create` (forced admin)
  → auto-`login()` → cookie → `/timeline`. 409 ⇒ another admin won the
  race: fall back to `/login` with notice. Any other probe result ⇒
  normal login. No album/config access before enrollment completes.
- `authenticate` open-mint stays but UI never calls it in enroll mode.

### C. After first user exists

- Window closes (store non-empty). Normal login; admin-less non-empty
  stays closed (CLI recovery). Legacy migration unchanged.

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

## Test cases (backend matrix, `enroll_gate.rs`)

1. Flag off, empty: `create`/`authenticate`/`get/users` → 401, count 0.
2. Flag on, empty: probe `[]` → create forces admin → second create 401
   → login 200.
3. Flag on, seeded non-admin: create 401, count unchanged.
4. Legacy + flag: guards 401, create 401, legacy login migrates, works.
5. Password-PUT empty store, no cookie: 404 (after revert), no user made.
6. Freshness predicate + boot-gate on real `passwd.json` (exists).

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

## Decisions

- Start is always the admin registration screen (explicit username —
  never an invisible `admin`).
- Gate stays "flag ∧ empty store" (not has-admin); admin-less
  non-empty recovers via CLI only.
- No new status endpoint; probe is `GET /get/users`.
