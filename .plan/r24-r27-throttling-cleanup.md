---
status: open
type: chore
priority: high
area: frontend
---

## Notes

### Problem

API-error display in the SPA is handled in one central place that does not
own it alone, and a spam guard covers two different jobs. Concretely:

- A one-second throttle guard in the axios response interceptor
  (`axiosInterceptor.ts`) is placed before BOTH of its two jobs for an
  unauthorized response (401): showing the "session expired" Vuetify
  snackbar and navigating to login. The profile is cleared before the
  guard (correct), but every 401 inside one second after the previous one
  is fully swallowed — no snackbar, no redirect. Parallel requests after a
  session death are the normal case, so signaling usually happens exactly
  once no matter how many denials arrive.
- Display ownership is split between the interceptor and per-screen error
  handling. The convention "axios errors were already displayed centrally,
  swallow them locally" exists in `tryWithMessageStore`
  (`script/utils/try_catch.ts`), but the register page's error callback
  (`RegistrationPage.vue` via `script/utils/registration.ts`) shows a
  second snackbar for axios errors, the upload store
  (`store/uploadStore.ts`) shows snackbars for every upload failure
  including already-displayed ones, and the share-password modal
  (`ShareLoginModal.vue`) shows an additional snackbar on top of the
  interceptor's own 403 branch. The poll-refresh page
  (`UserManagement.vue`) is the correct single-owner template; the others
  predate or bypass it.
- A rate-limited login (429) on the login endpoint has a designed silence
  pass-through: `LoginPage.vue` owns the inline countdown banner, the
  interceptor stays silent for that route. The hand-off only works when
  `retryAfterSecs` (`script/utils/retryAfter.ts`) can parse a wait time
  from the `Retry-After` header or response body. When it cannot, the
  login page rethrows, the interceptor stays silent by design, and
  `tryWithMessageStore` discards the rejection as "already displayed" —
  the user clicks login and gets zero feedback.
- The web-worker HTTP layer owns no auth UX: its interceptor
  (`worker/workerAxiosInterceptor.ts`) handles only server errors (500),
  so worker 401/429 rejections have no defined signal path.
- None of the 401 paths have tests: `axiosInterceptor.test.ts` covers only
  429 behavior.
- Backend echo of the same pattern: the malformed-body catcher
  (`router/catchers.rs` `unprocessable_entity`) replaces every 422 with one
  fixed sentence and loses all field detail; the shared "unauthorized"
  response component (`openapi_components.rs`) declares no response body
  while guard and handler 401s emit a JSON error payload; the catcher
  module comment overclaims when it fires; and the diagnostic route
  `GET /unauthorized` is prose-described as a page users "land" on although
  nothing navigates there (its wire shape — a bodiless status-only 401 —
  is accurately documented; the reachability prose and the generated
  `res.json()` sample are wrong).
- Evidence: triaged findings in `/tmp/opencode/r24-r27-review.md`
  (32 claims; 29 confirmed, 1 partial, 2 rejected — the rejected two claim
  a bare-guard detail loss and a forwarded-401-masks-as-200, both refuted
  against `backend/src/router/mod.rs` typing and the Rocket catch-all
  dispatch).

### Strategy

Frontend first, one seam at a time: throttle scope, then display-ownership
cleanup, then the hint-less 429 fallback, then the worker ownership
decision; backend error-contract work last because it is independent and
lower value. Rejected directions: per-screen throttle guards (recreates
the ownership split); making the interceptor silent everywhere (removes
the only signal on routes without an inline banner); parsing 422 detail
per-route without dispatch-level assertions (reproduces the
status-code-only test gap that hides the loss today).

### Open decisions (need answers before the respective steps)

- D1 — 401 snackbar wording (step T1): MOOT 2026-10-17 — the 401
  snackbar was deleted outright by the `login-session-banner` plan
  (the login-page inline banner owns the message; see T1 below). No
  wording decision needed.
- D2 — worker 401/429 ownership (step T4): forward the failure to the
  main thread (single global path continues) or document
  "workers never own auth UX; callers must handle" and pin it with a test.

## Implementation steps

Note: the `login-session-banner` plan, once implemented, deletes the 401
snackbar in favor of a login-page inline banner (Vuetify `v-alert`, the
inline non-dismissing banner) — re-check T1 scope before starting; T2
through T5 are unaffected.

### T1 — 401 rework (absorbed into the login banner plan)

Done 2026-10-17, absorbed: the `login-session-banner` plan did this work
in its steps 1-2 — the 401 branch now clears the profile unconditionally
and redirects once; the snackbar and the 1-second throttle guard are
deleted (no remaining consumer); four 401 behavior tests pin the contract
(commits `adee2358f`, `294355f7c`). The original throttle-relocation scope
became moot and the plain-worded summary of the landed state replaces it;
no scope remains under this item. T2, T3, T4, T5 are unaffected.

### T2 — single-owner display cleanup

Scope: `RegisterPage.vue` / `script/utils/registration.ts`,
`store/uploadStore.ts`, `ShareLoginModal.vue`, `UserManagement.vue` as
reference. Adopt the established convention everywhere: callers show a
snackbar only for non-axios errors or missing responses; the interceptor
owns axios-error display. Remove the per-caller 401 navigation
(`UserManagement.vue` duplicates the interceptor's redirect). Done: no
double snackbar and no doubled navigation on the register, upload, and
share-403 paths; full frontend suite green. Leaves out: T3, T4, backend.

### T3 — hint-less 429 fallback

Scope: `LoginPage.vue` catch path, interceptor 429 branch, tests. When
`retryAfterSecs` returns null for a login-request 429, show a fallback
message instead of rethrowing into the designed silence, so the submit
click always gets feedback. Extend coverage across the
banner/interceptor hand-off. Done: a hint-less 429 shows a message; the
interceptor-continues-silent contract still holds; suites green. Leaves
out: T4.

### T4 — worker auth-error ownership

Scope: `worker/workerAxiosInterceptor.ts` and its callers. Implement the
D2 decision (forward to main thread, or document-and-pin); the change is
the cheaper coherent step with one pinning test. Done: worker 401/429 has
exactly one defined UX path. Leaves out: backend contract work (T5).

### T5 — backend error contract

Scope: `backend/src/router/catchers.rs`, `backend/src/openapi_components.rs`,
`backend/src/router/get/get_page.rs` docs, `backend/openapi.json`
regen. Settle the 422 detail decision (preserve field detail with
dispatch-level response-body assertions, or record the fixed-string 422
as intentional and fix every place that promises detail); add the JSON
error body to the shared "unauthorized" response component only (the
inline bodiless `GET /unauthorized` entry stays accurate as is); narrow
the catcher module comment to when it actually fires; reword the
`/unauthorized` prose (bodiless diagnostic probe; no navigation target;
fix the `res.json()` reference sample). Done: `cargo test -p picasu
--lib`, `just openapi-check` green, sanity counts updated if the handler
inventory moves. Leaves out: the auth-flow doc rewrite (separate docs
pass), admin panel work, recovery-token work.
