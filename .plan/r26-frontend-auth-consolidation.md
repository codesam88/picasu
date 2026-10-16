---
status: open
type: feature
priority: high
area: frontend
---

## Notes

R26 fix vehicle, narrowed to the session module (per the review's own
scoping: "R23 (+ R24/R26 narrowed to the session module), not the full
fragmentation cleanup at once"). Replaces the overloaded `GET
/get/users` boot probe with an explicit anonymous `GET /get/server-info`,
collapses the router guard to profile-fast-path + fresh ping, deletes
the probe cache and its reset discipline, and ties off anonymous `GET
/get/users` to 401 in all states. Separate `RegisterPage`/`LoginPage`
stay; `LoginPage` is unchanged (it queries nothing on mount — the guard
runs before every render including the initial load, so no mount fetch
is needed).

Decisions (settled 2026-10-16): endpoint named `/get/server-info`
returning `{ enrolled, enrollmentOpen, captcha: null }`, hand-built
fields only, `Cache-Control: no-store` (caching is frontend-absent by
construction: the guard pings only when sessionless, so there is nothing
to invalidate); gallery `name` deferred until the config knob exists;
guard reads `sessionStore.profile` as routing-grade fast path with the
401 interceptor as enforcer; open-mode `[]` on `/get/users` becomes 401
(simplification + attack surface, not purity).

## Review-item mapping (`.plan/auth-enrollment-review.md`)

Addressed here:

- R23 (S1/HIGH, triplication + probe overload) — fully: the signal
  replaces the probe, the guard collapses to one decision point, the
  cache deletes. This task's core.
- R26 (S2/HIGH, frontend auth layer fragmentation) — partially,
  session-module slice only: enrollment routing + session signal
  consolidated into guard + one API module. The full `api/users.ts` vs
  `api/config.ts` vs six-utils merge stays out.
- R1 (S0/LOW, stale probe bounce) — fully, by deletion: no cache means
  no stale bounce; the five R1 reset tests are scrapped as behavior
  deleted, not repurposed.

Explicitly not addressed (left open in the review):

- R24 (interceptor double-toast/throttle) — handling stays; only the
  reset import drops as a consequence of the deletion.
- R10 (closed-install UX/naming), R25 (register-then-login atomicity),
  R27 (422 detail), R30/R31 (module collapse/form tests) — untouched.
- Docs limited to `docs/auth.md` boot paragraph plus completion notes
  for R23/R1 in the review file.

## Strategy

Contract first (backend endpoint + shape), then the single consumer of
the shape (guard + pages), then deletion of the old world, then specs
and docs. The guard is deliberately not cached: one tiny anonymous GET
per sessionless navigation costs ~ms and removes the entire stale-cache
bug class instead of managing it. Rejected: moving the probe into
`LoginPage` (relocates the overload, keeps reset discipline); keeping
the cache "just for the guard" (reintroduces R1 through the back door);
closing over `GET /get/config` too (R13's leak, separate task).

## Code rework inventory

Delete:

- `frontend/src/script/utils/enrollment.ts` (probe, cache, destination
  mapping — all importers rewired first).
- `RegisterPage.onMounted` leave-redirect (guard absorbs it).
- All `resetEnrollmentProbe` imports/calls (`LoginPage`, `RegisterPage`,
  `logout.ts`, `axiosInterceptor.ts`).
- Open-mode anonymous `[]` on `GET /get/users` (handler returns 401 on
  empty store like every other guarded surface).

Rework:

- `backend/src/router/get/users.rs`: empty-store → 401, no other change.
- `frontend/src/route/routes.ts`: guard becomes share-exempt →
  profile-present allow → fresh `getServerInfo()` → register/login
  redirect (already-there allowed through).
- `frontend/src/api/`: new `serverInfo.ts` (`getServerInfo`, zod shape).
- `backend/src/router/get/`: new `server_info.rs` + route registration;
  `backend/openapi.json` + `docs/openapi-reference.md` regen; sanity
  count 73→74 (verify, currently 73).

Untouched: `LoginPage.vue` (zero changes), `sessionStore`,
`currentUser`, `registration.ts` (comment-only mentions),
`submitRegistration` flows, `listUsers`/`UserManagement`,
interceptor 401 handling minus the reset import, `GET /get/config`.

## Test plan (planned separately from code)

### A. New semantics needing coverage

Backend (`server_info.rs` module tests):

- S1 anonymous 200 with exactly `{ enrolled, enrollmentOpen, captcha }`
  and nothing else (key-allowlist assertion — R13 regression pin).
- S2 empty+flag → `{ false, true }`; S3 empty+flag-off → `{ false,
false }`; S4 non-empty → `{ true, false }` regardless of flag.
- S5 `/get/users` empty store + flag on → 401 (tie-off).
- S6 enroll_gate rows rewritten: open-window enrollment proven via
  server-info, not probe `[]`.

Frontend:

- S7 guard with profile present performs zero fetches and allows.
- S8 guard sessionless + needs-enrollment → `/register`; already there
  → allows.
- S9 guard sessionless + enrolled → `/login`; `/register` → `/login`
  (absorbs the deleted leave-redirect); already on `/login` → allows.
- S10 share routes never redirected (kept).
- S11 `getServerInfo` parses shape, rejects malformed.
- S12 live Playwright first-admin matrix rewritten around server-info
  (anonymous shape per state + authenticated list still works).

### B. Old tests: scrap / update / keep

| File                                                                                                                                                          | Disposition                             |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------- |
| `enrollment.test.ts` (decide/probe/cache suites)                                                                                                              | SCRAP whole file with the module        |
| `registerRoute.test.ts` probe/cache/R1 tests (`probes enrollment…`, `shares cached probe…`, `LoginPage resets…`, `logout resets…`, `401 interceptor clears…`) | SCRAP                                   |
| `registerRoute.test.ts` (`defines /register`, `wires route`, `fields`, `share exemption`)                                                                     | KEEP; ADD S7–S10                        |
| `RegisterPage leaves when already enrolled`                                                                                                                   | SCRAP (behavior deleted; covered by S9) |
| `logout.test.ts` probe-reset test                                                                                                                             | SCRAP; rest KEEP                        |
| `enroll_gate.rs` open-window tests (2 pinning `[]`)                                                                                                           | UPDATE to S5/S6                         |
| `enroll_gate.rs` closed-install 401 tests                                                                                                                     | KEEP (still 401)                        |
| `users.rs` module + `sessions.rs` tests                                                                                                                       | KEEP, must stay green                   |
| `sessionStore/logout/currentUser/registration/api` suites                                                                                                     | KEEP, must stay green                   |
| first-admin Playwright steps 2–3, step-8 comment                                                                                                              | REWRITE per S12                         |

## Implementation steps (iterative TDD)

1. Backend endpoint (scope: `get/server_info.rs`, `get/mod.rs`,
   artifacts). Done 2026-10-16 (commit `c89fc1cea`: anonymous shape +
   no-store, 5 module tests, 633 lib green, 74 annotations).
2. Tie-off + enroll_gate (scope: `get/users.rs` handler,
   `enroll_gate.rs`). Done 2026-10-16 (commit `a9acecc83`: empty→401,
   open-window rows onto server-info; plus 2 r6 warning cleanups).
3. Frontend API + guard (scope: `api/serverInfo.ts`, `routes.ts`).
   Done 2026-10-16 (commit `1dbd5d094`: S7–S11 green; senior review
   hardened register→login redirect for sessionless visits).
4. Deletion sweep (scope: `enrollment.ts`, reset call sites,
   `RegisterPage.onMounted`, test dispositions above). Done 2026-10-16
   (same commit: 149 vitest green, tsc/eslint/prettier clean).
5. Specs live (scope: first-admin spec rewrite, fresh `vite build`,
   live run). Done: S12 green live. Leaves out: docs.
6. Docs + full gate (scope: `docs/auth.md`, review completion notes
   for R23/R1). Done: backend `cargo test`, frontend suite, all `just
check` gates, openapi suite green; interpreter auth scenarios green
   live (anonymous-media fallout stays a separate task).
