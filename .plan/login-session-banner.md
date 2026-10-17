---
status: in-progress
type: feature
priority: medium
area: frontend
---

## Notes

### Problem

When an API call fails with an unauthorized response (401), the user gets a
2.5-second transient Vuetify snackbar (`v-snackbar-queue`, the auto-dismissing
notification widget in `App.vue`) saying "Session expired or unauthorized" and
is simultaneously redirected to the login page. The message is transient,
competes with the page transition, and asserts a session was expired — which is
not always true: the visitor may simply have followed a deep link or bookmark
with no session to expire. Decided 2026-10-17: replace that snackbar with an
inline banner on the login page, worded innocuously so it is truthful for both
session-loss arrivals and no-session arrivals.

### Strategy

Option A (login-page inline banner) with innocuous wording; the second
notification concern raised the same day — transient action-failure feedback
(move-image/move-album) — is explicitly deferred. Vehicle: Vuetify
`v-alert` (the inline, non-auto-dismissing colored banner component) on
`LoginPage.vue`, visually consistent with the existing 429 countdown banner
from the rate-limiting work. Banner visibility decided 2026-10-17 (D4):
static — shown on every visit to the login page, no redirection-store
plumbing; truthful for session-loss and no-session arrivals alike
(rejected: reason-gating via a redirect flag — extra plumbing for no UX
gain at innocuous wording). Rejected: longer snackbar timeout (still
transient, still competes with navigation); asserting "session expired"
unconditionally (false for deep-link arrivals); no message at all (loses
the why).

## Implementation steps

1. Banner (scope: `LoginPage.vue`): add the static innocuous banner
   ("Please log in to continue"; exact copy settled in review). Vitest:
   banner renders on every login-page visit; existing login tests
   unaffected. Done: `just frontend-vitest`, `just frontend-check` green.
2. Snack-down removal of the 401 snackbar (scope:
   `axiosInterceptor.ts` 401 branch). With the banner owning the "why,"
   the reducer of this branch becomes: clear profile (unconditional,
   already ordered first), navigate to login (the redirection store
   deduplicates repeated navigations). The 1-second throttle guard loses
   its last snackbar to suppress — evaluate deleting it rather than
   gating a now-nonexistent message; delete if no remaining consumer.
   Must also drop the "Session expired" snackbar from the anti-pattern
   list of the r24-r27 plan (T1/T2 re-scoped: their double-display tests
   only apply to other error classes). Vitest: 401 → profile cleared +
   one navigation + no snackbar; banner visible after arrival.
   Done: suites green.
3. Docs + plan tie-off (scope: `docs/auth.md` login section, this plan):
   describe the banner as the 401-arrival feedback (Vuetify `v-alert`,
   inline; non-dismissing), note the innocuous wording rationale.
   Cross-check `r24-r27-throttling-cleanup.md` for the re-scoped T1.
   Done: `just docs-check` green, cross-refs consistent.

Knock-on recorded for the cleanup plan: the 401 Vuetify snackbar is deleted
here; T1's throttle-scope fix and T2's caller audit continue to own other
error classes (403/429/4xx), unchanged.
