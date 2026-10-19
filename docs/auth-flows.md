# Authentication Flows

How authentication is used, from the user's perspective: the desired
sequences (frontend + API interaction), the precise API semantics derived
from them, and the server-side enforcement rules. `docs/auth.md` describes
the mechanism; `docs/authz-model.md` states the invariants; this document
states the intended behavior at the user/API boundary.

## User journeys

### J1 — First startup (empty store)

The store has no users. The browser holds no token, so the user panel is
hidden until sign-in.

1. User opens the app. With the store empty, the SPA's boot guard pings
   `GET /get/server-info`, which answers `{enrolled: false, enrollmentOpen,
captcha: null}`:
   - `enrollmentOpen: true` (empty store + `--bootstrap-admin`) → the app
     shows the admin registration screen (username, password, confirm).
   - `enrollmentOpen: false` (flag off, empty or non-empty) → the app falls
     through to `/login`; every guarded route answers 401 `no administrator
enrolled` (J2).
2. Registering submits the new administrator (any id — never a forced
   invisible name) and auto-logins:
   - API: `POST /post/users/bootstrap {userId, password}` → 200. This is
     the only online path that can create an admin: the request body has
     no `admin` field, and the user is always forced `admin: true`.
   - The page then issues `POST /post/authenticate` with the entered
     credentials and receives the 14-day session cookie. A failed enroll
     (409 — another administrator won the first-create race) falls back
     to `/login` with a notice.
3. All further access uses the enrolled identity: the session was minted
   by the login round-trip in step 2, and guarded routes re-check the
   user's live record on every request.

### J2 — Closed install (store empty and `--bootstrap-admin` off)

1. Every guarded route answers 401 with the same refusal message, and
   `POST /post/authenticate` accepts nothing (the login page shows the
   ordinary form; any attempt is refused). There is no enrollment
   surface online — an unauthenticated caller cannot reach the user
   panel, list users, or log in. The browser falls through to `/login`.
2. Recovery is operator-only: restart with `--bootstrap-admin` on the
   empty store (J1, step 2), or provision via the `picasu-admin` CLI.
   `POST /post/users/register` can still create non-admin accounts while
   registration is open or a valid invite exists, but no online path ever
   grants admin — an admin-less store gains an administrator only through
   the CLI.

### J3 — Admin adds a user (API only — no UI yet)

1. Admin lists users: `GET /get/users` → `[{userId, admin}]` (hashes never
   included).
2. Admin registers the user: `POST /post/users/register {userId,
password}` → 200; the account is always `admin: false` (the body has no
   `admin` field; elevation is `PUT /put/users/admin`); duplicate id → 409. With the `allowRegistration` knob off, the admin session is the
   gate that skips the invite; a non-admin or anonymous caller needs a
   valid invite (403 registration disabled / 404 unknown, revoked or
   expired invite token).
3. The new user logs in (J4). There is currently no create-user form in
   the frontend; this flow is API-only (gap G1).

### J4 — Login and logout

- Login: `POST /post/authenticate {userId, password}` → 200 + session
  profile `{ userId, admin }` in the body; the 14-day JWT travels in an
  HttpOnly `Set-Cookie` the page never sees (R5). Unknown user and
  wrong password both answer 401 with no distinguishing detail, and the
  unknown-user path still costs one KDF round (no timing oracle).
- Logout is `POST /post/logout` → 204: it removes the presented `jti`
  from the caller's live sessions and clears the cookie server-side
  (idempotent); the client also drops its session profile and capability
  tokens. Only the presented device is logged out — other sessions
  survive, and they remain listed under `GET /get/sessions` until expiry.

### J5 — Password change

- Self-service: the user supplies the correct old password
  (`PUT /put/users/password {userId: self, oldPassword, newPassword}`).
- Admin reset: an admin sets any user's password without the old one.
- Passwords are trimmed on write and on verify (one canonical form).
- Outstanding sessions die with the change: every `jti` of the target is
  wiped before the new hash is written, so all devices re-login with the
  new password.
- Empty new password → 400. There is no "clear to open": open mode is
  store-empty only.

### J6 — Role transfer

1. Admin A promotes B (Settings toggle with confirm, or
   `PUT /put/users/admin {userId: B, admin: true}`).
2. B logs in again — promotion requires re-login (the token-embedded flag
   gates before the DB re-read).
3. A demotes self (allowed while other admins exist, with an explicit
   self-demotion confirm in the UI).
4. Refusals: demoting the sole admin (self or other) → 409; the check and
   the write are one atomic transaction, so concurrent demotions cannot
   strand zero admins. Every change writes one audit log line
   (actor, target, new flag).

### J7 — Guest access (no login)

- Anonymous share link: `POST /get/prefetch` + `GET /get/get-data` +
  media serving with `x-album-id`/`x-share-id` headers (or
  `albumId`/`shareId` query), no cookie. Password-protected shares add the
  `x-share-password` header.
- A logged-in non-admin with share headers gets the share's scoped access;
  without them, share-guarded routes are 401. An admin-role user passes
  share-guarded routes without share headers.
- Demotion never affects share capabilities already minted (valid to `exp`);
  it only blocks new identity and renewal.

## API semantics

| Endpoint                     | Auth                                                                                                                                                                    | Success                                                                                                                                                                                                                                                                          | Refusals                                                                                                                                                                  |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `POST /post/authenticate`    | none                                                                                                                                                                    | 200 + session profile `{userId, admin}`; 14-day JWT in an HttpOnly `Set-Cookie`. Empty store + `--bootstrap-admin` → open mode: any parseable body is accepted (ephemeral admin, creates nothing). Else verify vs. the password store → 401 unknown/wrong (generic, +dummy KDF). | 400 bad id/body; 401; 429 throttled (+`Retry-After`)                                                                                                                      |
| `POST /post/users/bootstrap` | none while the window is open (`--bootstrap-admin` ∧ empty store); a shut window answers 401 (an authenticated caller falls through to the same missing-enrollment 401) | 200 + `{userId, admin: true}` (forced)                                                                                                                                                                                                                                           | 400 bad id/weak pw; 401 window shut; 405 read-only; 409 duplicate; 500 storage                                                                                            |
| `POST /post/users/register`  | admin session, or none with `allowRegistration` on, or a valid unexpired unused invite; always non-admin, never the first user (empty store → 401)                      | 200 + `{userId, admin: false}`                                                                                                                                                                                                                                                   | 400 bad id/weak pw; 401 empty store; 403 registration disabled or invite expired; 404 unknown/revoked invite; 405 read-only; 409 duplicate id or used invite; 500 storage |
| `GET /get/users`             | admin                                                                                                                                                                   | 200 list, no hashes                                                                                                                                                                                                                                                              | 401                                                                                                                                                                       |
| `PUT /put/users/password`    | any authenticated user                                                                                                                                                  | 200. Admin: any target, no old needed. Self: correct old required.                                                                                                                                                                                                               | 400 empty new/wrong old; 401 no session; 403 closed install or cross-user non-admin; 404 unknown (non-empty store); 405 read-only                                         |
| `PUT /put/users/admin`       | admin                                                                                                                                                                   | 200 (no-op same-flag included)                                                                                                                                                                                                                                                   | 400 bad id; 401; 404 unknown; 405 read-only; 409 zero-admin outcome                                                                                                       |
| share-guarded reads          | share headers/query or admin cookie                                                                                                                                     | scoped data                                                                                                                                                                                                                                                                      | 401; 403/404 on scope violations (F2/F3 fixes)                                                                                                                            |
| `POST /upload`               | share with `showUpload` + matching album, or admin                                                                                                                      | accepted                                                                                                                                                                                                                                                                         | 401                                                                                                                                                                       |

## Enforcement rules (server-side, test-pinned)

- **R1 — zero-admin refusal** is one atomic transaction (check + write, single
  redb writer). Pinned: sole-admin self-demote 409, sole-other demote 409,
  transfer-then-demote 200.
- **R2 — bootstrap is one-shot.** Empty-store creation/migration paths 401
  once any user exists. Pinned: second unauthenticated create 401, second
  legacy attempt 401.
- **R3 — deny is live, grant needs re-login.** Demotion/removal is honored
  before token expiry (per-request re-read); promotion mints only via fresh
  login. Pinned both directions.
- **R4 — trim canonicalization** on write and verify. Pinned.
- **R5 — no oracles.** Unknown user ≡ wrong password (401 + one KDF round);
  permission checked before existence (403 before 404). Pinned.
- **R6 — hashes never leave the server.** Separate file, atomic writes,
  `0600`, excluded from export and list responses. Pinned (list body has no
  hash/salt fields).
- **R7 — every role change is audited** (actor, target, flag).
- **R8 — read-only blocks all mutations** (405), including user management.
- **R9 — tokens are typed and bound** (`typ`, witness consumption, scope;
  see `authz-model.md` AUTH-1/4/6–8).

## Coverage map (use-case → tests)

| Use-case                                            | Tests                                                                                                                  |
| --------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| J1 bootstrap + forced admin + one-shot              | `auth_users::bootstrap_open_mode_and_first_user_forced_admin`, `post/users.rs::bootstrap_create_forces_admin`          |
| J2 legacy object bootstrap + string migration       | `authenticate.rs::legacy_login_registers_claimed_id_as_admin`, `legacy_string_migrates_to_admin_user`, rejection twins |
| J3 create/list/duplicates                           | `post/users.rs` (5), `get/users.rs` (3)                                                                                |
| J4 login right/wrong/unknown/trim                   | `authenticate.rs` (9), `auth_users::two_users…`                                                                        |
| J5 self/admin/legacy/empty/403-ordering             | `users_password.rs` (7+1), `edit_config.rs` legacy tests                                                               |
| J6 transfer/promotion/demotion/liveness             | `users_admin.rs` (10), `auth_users::password_change_and_demotion…`                                                     |
| J7 anonymous + role interplay + scope               | `auth_users::guest_share…`, `authz.rs` (7)                                                                             |
| Liveness (demoted/removed/unknown/non-admin tokens) | `auth.rs::jwt_auth_*` (5)                                                                                              |
| KDF/file store unit properties                      | `auth/password.rs` (10), `auth/users.rs` (7+4)                                                                         |

## Gaps (no API tests)

- **G1 — create-user UI missing.** J3 is API-only; backend covered, no
  frontend path exists.
- **G2 — read-only 405 untested** for create, set-password (both routes),
  legacy-password route, and login-while-read-only (should still work).
  Only set-admin pins 405.
- **G3 — password-protected share flow** end-to-end (share with password →
  correct/wrong/missing `x-share-password`).
- **G4 — identity-token expiry** → 401 (craft an expired token; timestamp
  tokens have the mint-expired helper, identity tokens do not).
- **G5 — token survives password change** (documented behavior, unpinned).
- **G6 — login fallback chain against a live backend** (object→string→object
  covered only with mocked HTTP; Playwright login scenarios updated but
  unexecuted).
- **G7 — concurrent bootstrap race** (by construction via mutex; accepted
  untested — record, do not chase deterministically).
- **G8 — share-credential callers on create/list/password routes** (only
  set-admin pins the share-headers 401).
- **G9 — demoted user's share access still works** (by design — share caps
  are independent; worth pinning so a future change cannot silently couple
  them).
- **G10 — open-mode bare-string acceptance** (implicit; no explicit test).
