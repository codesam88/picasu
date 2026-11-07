---
status: backlog
type: chore
priority: low
area: frontend
---

## Notes

Migrate the custom `scrollBehavior` in `frontend/src/route/routes.ts`
(restore saved position, else scroll top) to `ScrollRestoration` /
`useScrollRestoration`. Blocked: the replacement lives in
`vue-router/experimental` (installed vue-router 5.4.0) and may churn.

## Steps

1. Migrate once the API stabilizes out of experimental.
2. Verify scroll restoration via the e2e suite (silent-failure risk —
   unit tests cannot catch it).
3. Remove the targeted `no-deprecated` disable at `routes.ts`.

## Out of scope

Current function is small and correct; no user-facing gain, so no rush.
