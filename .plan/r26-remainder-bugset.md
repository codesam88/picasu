---
status: done
type: bug
priority: high
area: full-stack
---

# R26 remainder bug set (401 taxonomy, display ownership, guard coverage)

Supersedes the R26 remainder tracking previously folded into
`.plan/auth-enrollment-review.md` (its R26 entry now points here). The
finding source is the two-round R26 review run of 2026-10-18; the
`.plan/r26-frontend-auth-consolidation.md` session-module slice and
`.plan/r24-r27-throttling-cleanup.md` T1–T5 remain the settled canon.

## Problem (consolidated, verified)

### B1 — non-session 401s end the SPA display session (S2/MED)

The interceptor's 401 branch is URL-blind, so _every_ response 401
clears the display profile and redirects — including 401s that did not
come from the session tier. The backend emits four families under one
`ErrorKind::Auth` umbrella (`error.rs:138` maps kind → 401):

| Situation                                                                                                                                                                                    | Today                                                                                             | Correct tier                                                                                                 |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Cookie/session guard rejection (expired, revoked, absent)                                                                                                                                    | 401 + **fixed** body `"Unauthorized"` (catcher `catchers.rs:32`, contract via `UnauthorizedBody`) | session — `/login` redirect is right                                                                         |
| Privilege misuse: non-admin PUT on another user's password (`GuardUser::denied`, `auth.rs:875-885`), also the no-admin-enrolled closed-install case                                          | 401 + human message                                                                               | privilege → 403 (`PermissionDenied` already maps to Forbidden, `error.rs:137`)                               |
| Value-credential failure: wrong _current password_ on self-change (`users_password.rs:81`, pinned by `self_service_with_wrong_old_password_is_401` + `admin_self_change_without_old_is_401`) | 401 + `"Incorrect current password"`                                                              | input validation → 400 (`InvalidInput`) — the request's own credential was accepted; the body value is wrong |
| Login credential failure (`authenticate.rs:292`, `"Invalid credentials"`, throttle reservation released on it)                                                                               | 401                                                                                               | stays 401 — the presented body password **is** the credential; caller-owned display                          |

Neither of the first two middle families invalidates the cookie — but
the client-side destruction is durable: `clearProfile()` removes
`SESSION_PROFILE_KEY` from localStorage and nothing rebuilds the display
profile except a full re-login (`sessionStore.ts`). So a wrong current
password, or a non-admin attempting a privilege action the SPA's own UI
already gates, silently logs the user out of the SPA while the cookie
remains valid (`routes.ts:135` treats the profile-less display as
sessionless). No test pins an authenticate-401 or password-401 case on
the interceptor today (401 coverage is `/get/get-data` only,
`axiosInterceptor.test.ts:152-191`).

**Settled solution (O2, approved 2026-10-18) — retype to the true
taxonomy and make 401 session-terminal by invariant:**

> 401 ⇒ the presented credential (cookie) was not accepted ⇒
> session-tier ⇒ clear display profile + redirect. Everything else is
> not a 401.

The interceptor then needs token-URL knowledge for exactly one case
(failed login on `/login`, whose 401 is legitimate but caller-owned),
the same carve-out shape the 429 branch already uses. This generalizes
the classification the worker tier already performs (serving-token
`tokenExpired` → renew vs unexpected → `unauthorized` channel).

### B2 — double snackbar on share creation (S2)

`CreateShareModal.vue:80-82` catch-all toasts on axios errors the
interceptor already displayed (`axiosInterceptor.ts:122-127` default
branch); the same file's `updateLink` (`:98`) already uses the
`tryWithMessageStore` guarded pattern. Settled convention: the
interceptor owns axios-error display.

Fix: route `createLink` through `tryWithMessageStore`; one behavior
test.

### B3 — boot guard is behavior-untested (S2 assurance)

`routes.ts:128-146` is asserted only via source-string regexes
(`registerRoute.test.ts:51-107`). No failing/refused
`GET /get/server-info` scenario exists in any layer, and the guard has
no error path — a rejected ping aborts navigation untested.

Fix: extract the guard decision into an injectable pure module;
behavior tests for profile-present, needs-enrollment, enrolled-redirect,
share-exempt, and ping-rejection → degrade. Keep the existing regex pins
as tripwires (update minimally, treat as R31-rewrite candidates).

### B4 — `/post/authenticate` literal ×3 (latent drift bug)

`axiosInterceptor.ts:11`, `script/utils/retryAfter.ts:7` (export, used
in exact-string compares at `:42`), `api/users.ts:67` (transport seam,
pinned by `users.test.ts:93`). Drift fails silently: the throttle toast
reappears alongside the login banner.

Fix: `axiosInterceptor.ts` imports `AUTHENTICATE_URL` from
`retryAfter.ts` (no cycle — verified); `users.ts:67` keeps its
route-derived literal. −3 lines, zero test changes. Do together with B1
(same test file, same branch family).

### B5 — phantom endpoint call (S3)

`store/shareStore.ts:29-40` `fetchAllShares()` calls
`GET /get/get-all-shares`, which exists nowhere (backend tree,
`openapi.json`, reference docs). Action and `allShares`/`fetched` state
are caller-less. Caution: `resolvedShare` in the same store IS live
(GalleryShareBar, ViewPage, DropZoneModal, MetadataContent) — delete
only the dead members.

Fix: delete `fetchAllShares`/`allShares`/`fetched`; no behavior change.

## Record-only items (no work here)

- `createUser` (`api/users.ts:17-23`) stays as-is: backend route is live
  but gated (`register.rs:129-135`); the wire-or-delete decision remains
  open until the admin-create form lands. Keep the
  `users.test.ts:52-63` pin.
- `resetAuthKey` placement in `config.ts` and the missing HTTP-placement
  convention are documentation items, not defects.
- S3 cleanup tail (ceremony dup, dead `exportConfig`/`importConfig`
  bindings, stale comments/tripwires, docs staleness) belongs to
  `.plan/frontend-vue-consolidation.md` and the auth-enrollment-review
  docs pass, not here.

## Implementation steps (iterative TDD)

(tier order: backend retyping first so the 401 invariant holds on the
wire, then the frontend consumes it)

### Step 1 — B1(+b): backend taxonomy retype (contract change)

Scope: `backend/src/router/auth.rs` (`GuardUser::denied`),
`backend/src/router/put/users_password.rs`, error-kind mappings in
`backend/src/error.rs` (mapping already exists — no change expected),
OpenAPI annotations + `backend/openapi.json` + generated reference,
`docs/auth.md` / `docs/config.md` mentions of the affected statuses.

1. `GuardUser::denied` (privilege + no-admin-enrolled cases):
   `ErrorKind::PermissionDenied` → 403. Keep the human message text;
   add the machine tier via the existing kind machinery. RED first:
   a module test asserting the non-admin-PUT-on-other case is 401
   today flips to `403`.

   Done (2026-10-18), with two corrections against the tree: the
   non-admin-PUT-on-other case was already 403 (`PermissionDenied` in
   `users_password.rs`, pinned by `cross_user_by_non_admin_is_403`) —
   the actual remaining privilege case on the wire was the GuardUser
   no-admin-enrolled branch, now 403 JSON (kind + enrollment hint). A
   bare guard's `AppError` payload is discarded by Rocket codegen
   (status only → catchers), so the route consumes the guard via
   `GuardResult<GuardUser>` (dominant house pattern) to keep the 403
   reason-bearing; catchers untouched.

2. `users_password.rs` wrong-current-password: `ErrorKind::InvalidInput`
   → 400. This reverses a pinned design — update both module tests
   (`self_service_with_wrong_old_password_is_401`,
   `admin_self_change_without_old_is_401` — the latter encodes the same
   value-failure semantics for the missing-old case) and the endpoint's
   `401` OpenAPI annotation + doc-comment rationale (`:39`, `:113`).
3. Session tier untouched: guards keep the fixed `"Unauthorized"` body;
   the catcher keeps its fixed string. Add one named test/comment pin:
   guards never emit reason-bearing 401 bodies; handler-raised Auth
   401s only reach the login endpoint after this change.
4. Regenerate `openapi.json` + widdershins reference
   (`just docs-openapi`), rerun `just check` (backend), audit the YAML
   scenarios / e2e for any password-401 expectations flipped by this.

Rejected here: adding a machine `code` field to `AppError` (O3) — the
existing kind level already carries the tier; revisit only when external
API consumers are planned.

### Step 2 — B1(+a): frontend consumes the invariant

Scope: `frontend/src/axiosInterceptor.ts` + `axiosInterceptor.test.ts`,
`frontend/src/script/utils/passwordChange.ts` flow display check,
`changePasswordForm` display wiring.

1. RED: authenticate-401 test (signed-in user, failed login) clears the
   profile today.
2. GREEN: 401 branch gains the `AUTHENTICATE_URL` carve-out mirroring
   the 429 branch — a login-401 never clears the profile; the login
   page owns the display. After step 1 every _other_ handler-401 has
   been retyped away, so the remaining 401 set is uniformly
   session-tier: keep `clearProfile()` + redirect unconditional in that
   set. B4 folds in here (the constant import from `retryAfter.ts`
   removes the URL-literal triplication).
3. Password-change flow: wrong-current-password now arrives as 400 —
   display comes from the interceptor's default branch (+ legacy
   `errorDisplay` mapping); confirm no caller-side handler keys on the
   old 401 (`passwordChange.ts` has none today — behavior display only).
4. Privilege case has no SPA caller (no UI path to administer another
   user from the self-change modal); no new branch needed — the
   existing 403 branch (`"Access denied."`) covers the future admin
   panel case.
5. With the invariant holding, drop the 401 branch's speculative
   URL-knowledge to the single shared carve-out; net frontend delta is
   small and the interceptor's semantics become "session-tier only".

### Step 3 — B2: single-owner share-create display

Scope: `frontend/src/components/Modal/CreateShareModal.vue` + a
behavior test. Route `createLink` through `tryWithMessageStore`
(`updateLink` at `:98` is the in-file template).

### Step 4 — B3: guard decision module with behavior tests

Scope: extract the `routes.ts:128-146` guard decision into an
injectable pure module; behavior cases: profile-present, needs-
enrollment, enrolled-redirect, share-exempt, ping-rejection → degrade.
Existing regex pins (S7–S10) shrink to tripwires; note them as R31
rewrite candidates.

### Step 5 — B5: delete phantom share-store members

Scope: `frontend/src/store/shareStore.ts` only — delete
`fetchAllShares`/`allShares`/`fetched`; grep-verify `resolvedShare`
consumers untouched.

### Step 6 — plan-text sync

Update `.plan/auth-enrollment-review.md` (R26 entry + follow-up):
record the O2 retype, disposition chain, and pointer here.

## Outcome (executed 2026-10-18; commits `7728075d4`…`c8d4147f1`)

- Step 1 (backend retype): see the annotation under item 1 above;
  additionally both password tests flipped to `..._is_400`, the
  endpoint's OpenAPI/doc errors were retyped (its only 401 source is
  now the session guard), `sessions.rs` declares the newly-possible
  403, openapi + reference regenerated. The plan's "guards emit the
  fixed `Unauthorized` body" claim was inverted by measurement: bare
  guard payloads are discarded by Rocket codegen (the status reaches
  catchers, so a bare-guard 403 would render HTML); reason-bearing
  bodies come from `GuardResult` routes, which is why the password
  route consumes its guard through `GuardResult`.
- Step 2: the 401 branch gained the `AUTHENTICATE_URL` carve-out
  (mirror of the 429 one); B4 folded in (single constant source,
  `retryAfter.ts`); the password-400 display path is test-pinned.
  "Login page owns the display" resolves to the static banner plus
  the inline 429 throttle — no 401 has produced a snackbar since
  `294355f7c`, which made the first-admin e2e spec's wrong-password
  step stale; that step was updated here (regression matrix scope).
- Step 3: `createLink` routed through `tryWithMessageStore`; display
  contract pinned structurally (vitest has no component mount
  harness — same pattern as `shareLoginModal.test.ts`).
- Step 4: decision extracted to `route/bootGuard.ts` with six
  behavior tests; a rejected ping degrades to the login fall-through
  (previously an unhandled rejection aborted navigation); the S7–S10
  regex pins shrank to tripwires (R31 rewrite candidates).
- Step 5: phantom members deleted; grep found no consumers and no
  backend route for `/get/get-all-shares`.

Record: pre-existing at execution time — `interpreter.spec.ts`
scenarios end on the Create Admin screen even at the pre-execution
baseline `2ee4ea835` (verified in a throwaway worktree), so it is not
regression from this bug set; root cause (boot-guard flow vs scenario
seeding) tracked in `.plan/e2e-boot-guard-scenario-red.md`.

## Verification

Backend per step 1: `cargo test -p picasu --lib`, `just openapi-check`,
`just check`. Frontend per steps 2-4: `just frontend-test` (vitest),
`just frontend-check` at step 2. Playwright: the guard-related suites as
a regression matrix at step 4; the first-admin-linked password flows at
step 1-2.

Out of scope: full api-layer merge (deferred by the session-slice
scoping decision), machine `code` field (O3), share-flow 401 handling
(split-modal owns it), Playwright spec changes beyond the regression
matrix, admin panel work, R31 test rewrites.
