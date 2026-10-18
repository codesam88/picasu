---
status: open
type: bug
priority: high
area: frontend
---

# R26 remainder bug set (interceptor 401, display ownership, guard coverage)

Supersedes the R26 remainder tracking previously folded into
`.plan/auth-enrollment-review.md` (its R26 entry now points here). The
finding source is the two-round R26 review run of 2026-10-18; the
`.plan/r26-frontend-auth-consolidation.md` session-module slice and
`.plan/r24-r27-throttling-cleanup.md` T1–T5 remain the settled canon.

## Problem (consolidated, verified)

### B1 — failed-login 401 ends the SPA display session (S2/MED)

The interceptor's 401 branch has no `AUTHENTICATE_URL` exclusion (only
the 429 branch does, `axiosInterceptor.ts:116`). A failed login on
`/login` while a valid-cookie user is signed in clears the display
profile (`clearProfile()`), while the backend authenticate error path
returns `Err` without cookie mutation (`backend/src/router/post/
authenticate.rs:429-458`). The guard then treats the profile-less user
as sessionless (`routes.ts:135`). A mistyped password silently logs the
user out of the SPA despite the cookie being valid. No test pins an
authenticate-401 case today (coverage is `/get/get-data` only,
`axiosInterceptor.test.ts:152-191`).

Fix: exclude `AUTHENTICATE_URL` from the 401 branch, mirroring the
settled 429 rationale (transient, not a dead session); the LoginPage
inline banner owns the message.

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

1. **B1 + B4** (scope: `frontend/src/axiosInterceptor.ts`,
   `axiosInterceptor.test.ts`). RED: authenticate-401 test clears the
   profile today; GREEN with the exclusion. Also add the constant import
   and pin the no-cycle/no-test-impact facts. Gate: full frontend suite
   green, `just frontend-check`.
2. **B2** (scope: `frontend/src/components/Modal/CreateShareModal.vue`
   - test). Follow the established `tryWithMessageStore` pattern;
     `updateLink` is the in-file template.
3. **B3** (scope: `frontend/src/route/` guard decision module +
   tests). Injected fetch; five behavior cases; regex pins updated as
   tripwires; backend keeps its own module tests untouched.
4. **B5** (scope: `frontend/src/store/shareStore.ts`). Delete dead
   members only; grep-verify `resolvedShare` consumers untouched.
5. Plan-text update in `.plan/auth-enrollment-review.md` (R26 entry +
   follow-up): premises corrected, remainder dispositions recorded,
   pointer here.

## Verification

Per step: `just frontend-test` (vitest) green; step 1 additionally
`just frontend-check`; final state additionally a manual Playwright
first-admin matrix re-run comment if the guard module changes

Out of scope: full api-layer merge (deferred by the session-slice
scoping decision), backend endpoint changes, Playwright spec changes
beyond the guard regression matrix, admin panel work.
