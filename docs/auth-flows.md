# Authentication Flows

How authentication is used, from the user's perspective: the desired
sequences (frontend + API interaction), the precise API semantics derived
from them, and the server-side enforcement rules. `docs/auth.md` describes
the mechanism; `docs/authz-model.md` states the invariants; this document
states the intended behavior at the user/API boundary.

## User journeys

### J1 — First startup (fresh install, empty store)

The server is open: no users, no legacy password. The browser holds no
token, so the user panel is hidden until sign-in.

1. User opens the app and logs in with any id + password (UI: Login page).
   - API: `POST /post/authenticate {userId, password}` → 200, admin token
     (open mode accepts any parseable body).
2. User opens Settings → User Management (now visible — the token carries
   the admin role) and creates their real admin account (any id; the first
   created user is forced `admin: true` regardless of the flag sent).
   - API: `POST /post/users/create` (unauthenticated bootstrap) → 200.
3. User logs out and back in as the new account. The open-mode token is
   discarded; all further access uses the created identity.

### J2 — Legacy upgrade (config password set, empty store)

1. User opens the app and logs in with a chosen User ID + the legacy
   password (UI: same Login form).
   - API: `POST /post/authenticate {userId, password}` → 200. The first
     login with the legacy password **registers the claimed id as the
     first admin**; the legacy password is then dead (further bare-string
     attempts are 401).
   - The UI fallback chain (object → string → object) is invisible; it only
     matters for bare-string API clients, which migrate onto `admin`.
2. Subsequent logins are normal user logins. The id typed at first login
   becomes the admin name — there is no rename, so it should be chosen
   deliberately.

### J3 — Admin adds a user (API only — no UI yet)

1. Admin lists users: `GET /get/users` → `[{userId, admin}]` (hashes never
   included).
2. Admin creates the user: `POST /post/users/create {userId, password,
admin}` → 200 (`admin` honored); duplicate id → 409.
3. The new user logs in (J4). There is currently no create-user form in the
   frontend; this flow is API-only (gap G1).

### J4 — Login and logout

- Login: `POST /post/authenticate {userId, password}` → 200 + 14-day token
  in the body, stored by the client in the `jwt` cookie. Unknown user and
  wrong password both answer 401 with no distinguishing detail, and the
  unknown-user path still costs one KDF round (no timing oracle).
- Logout is client-side only (cookie removed); the token stays valid to
  expiry. There is no server-side session to invalidate.

### J5 — Password change

- Self-service: the user supplies the correct old password
  (`PUT /put/users/password {userId: self, oldPassword, newPassword}`, or
  the legacy `PUT /put/config/password` route which is caller-only).
- Admin reset: an admin sets any user's password without the old one.
- Passwords are trimmed on write and on verify (one canonical form).
- A change takes effect on the next login; outstanding tokens survive it.
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

| Endpoint                         | Auth                                                    | Success                                                                                                                                                                                  | Refusals                                                                                                      |
| -------------------------------- | ------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `POST /post/authenticate` object | none                                                    | 200 + token. Empty store + legacy password + match → bootstrap claimed id as admin. Empty store, no legacy password → open token. Else verify → 401 unknown/wrong (generic, +dummy KDF). | 400 bad id/body; 401                                                                                          |
| `POST /post/authenticate` string | none                                                    | 200 only while store empty + legacy match (migrates onto `admin`)                                                                                                                        | 401 otherwise                                                                                                 |
| `POST /post/users/create`        | none iff store empty (forces `admin: true`); else admin | 200 + `{userId, admin}`                                                                                                                                                                  | 400 bad id/empty pw; 401 non-admin/anon-nonempty; 405 read-only; 409 duplicate                                |
| `GET /get/users`                 | admin                                                   | 200 list, no hashes                                                                                                                                                                      | 401                                                                                                           |
| `PUT /put/users/password`        | any authenticated user                                  | 200. Admin: any target, no old needed. Self: correct old required.                                                                                                                       | 400 empty new; 401 wrong old/no creds; 403 cross-user non-admin; 404 unknown (non-empty store); 405 read-only |
| `PUT /put/users/admin`           | admin                                                   | 200 (no-op same-flag included)                                                                                                                                                           | 400 bad id; 401; 404 unknown; 405 read-only; 409 zero-admin outcome                                           |
| `PUT /put/config/password`       | any authenticated user                                  | 200, caller-only (delegates to the same rules)                                                                                                                                           | as above; 404 only in the deleted-mid-flight race                                                             |
| share-guarded reads              | share headers/query or admin cookie                     | scoped data                                                                                                                                                                              | 401; 403/404 on scope violations (F2/F3 fixes)                                                                |
| `POST /upload`                   | share with `showUpload` + matching album, or admin      | accepted                                                                                                                                                                                 | 401                                                                                                           |

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
