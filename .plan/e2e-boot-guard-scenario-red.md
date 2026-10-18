---
status: backlog
type: bug
priority: medium
area: frontend
---

# Interpreter e2e scenarios end on the Create Admin screen (pre-existing red)

## Problem

The full Playwright suite on `auth-enrollment-review` is red well
before `.plan/r26-remainder-bugset.md` was executed: every
`interpreter.spec.ts` scenario fails with the page parked on the
Create Admin screen ("No administrator is enrolled yet. Register the
first one.") instead of the flow's expected page. Verified
pre-existing: reproduced identically in a throwaway worktree at
`2ee4ea835` (the tip just before the R26 bug-set backend commit).

Suspected area (not yet isolated): the sessionless boot guard
(`route/bootGuard.ts`, then inline in `routes.ts`) forces `/register`
whenever the fresh `GET /get/server-info` ping reports
`enrolled: false, enrollmentOpen: true`; scenario setup appears to hit
a navigation before the admin seeding lands, and/or the scenario
seed/login steps no longer match the post-`1dbd5d094` boot flow.

Failure count depends on worker parallelism: at the default invocation
(`npx playwright test tests/playwright/interpreter.spec.ts`) the count
was 33 failed / 6 passed both at baseline `2ee4ea835` and after the
R26 bug set; under `just frontend-playwright`'s `nproc/2` workers,
"Navigate to albums updates route" additionally flaked into the same
register-screen trap (passes in isolation). The trap itself is
deterministic for the 33.

## Bisect result (2026-10-18, deterministic single-scenario probes + two full-suite runs)

First bad commit: **`2ea82b58f`** — `feat(auth-ui): force first-admin
registration before app access` (2026-10-14 10:05).

| Commit                             | Full interpreter suite   | Notes                                                                                                                    |
| ---------------------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `2dc4ca114` (parent, 10-14 00:44)  | **35 passed / 4 failed** | 4 failures are the `user:`-given scenarios, failing fast (~1s, separate older defect; 3 later repaired by other commits) |
| `2ea82b58f` (guard, 10-14 10:05)   | **2 passed / 37 failed** | 33 error contexts are the register-screen trap                                                                           |
| `331641244` (session start, 10-18) | 33 failed / 6 passed     | same deterministic trap set                                                                                              |

Mechanism (verified):

1. `backendLauncher.ts` boots each scenario with
   `PICASU_BOOTSTRAP_ADMIN=true` and an empty user store.
2. `given:` steps are API-side; on an empty store with the flag on,
   `POST /post/authenticate` succeeds in open mode **without creating a
   user** (`authenticate.rs` `open_mode`), so the store stays empty for
   the ~35 scenarios without a `user:` given.
3. The boot guard (then: `GET /get/users` probe; now: `GET
/get/server-info` via `route/bootGuard.ts`) sees empty+open on the
   first sessionless navigation and forces `/register`, so every UI
   assertion lands on the Create Admin screen.

Why it went unnoticed: the precommit gates run vitest/clippy/eslint,
not Playwright — the introducing commit's own message reports "Full
frontend suite (157)" (vitest) as its green. `just test` does include
Playwright but is run manually.

Fix direction (proposal, pending review): harness-side session boot in
`interpreter.spec.ts` — enroll via API when the store is empty,
authenticate, then either inject the `jwt` cookie + display profile
(guard's fast path) or perform a real UI login before executing steps;
scenarios that manage their own session (the `user:`-given ones) opt
out. Product-side guard relaxation is rejected: the register-forcing
contract is deliberate and pinned (`bootGuard.test.ts`,
first-admin spec). Adjacent: `login-handling.yaml` fails independently
of the guard (pre-dates it) and needs its own diagnosis.

## Goal

`just frontend-playwright` green on the current branch: scenario YAMLs
either seed before the first guarded navigation or the interpreter's
login handling matches the current guard; no behavior regression in
the guard itself (its unit coverage lives in `bootGuard.test.ts`).

## Implementation steps

1. Isolate: run one scenario with tracing, find the first navigation
   that lands on `/register`, and compare against scenario setup order
   (seed → login → navigate vs navigate → guard ping).
2. Decide: adjust scenario seeding order / interpreter login step, or
   adjust the guard contract (only with a written rationale — the
   guard's register-forcing behavior is pinned by `bootGuard.test.ts`).
3. Regenerate scenario artifacts if YAMLs change
   (`docs/playwright_generator.md`).
4. Run the full suite as the regression matrix.

## Verification

`just frontend-playwright` passes fully; `just frontend-vitest` and
`just frontend-check` unaffected.

## Notes

Found while executing `.plan/r26-remainder-bugset.md` (record-only
there). Related but distinct: `first-admin-registration.spec.ts`'s
stale wrong-password snackbar expectation was already fixed in that
bug set (`51814fa09`).
