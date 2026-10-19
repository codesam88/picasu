---
status: done
type: bug
priority: medium
area: frontend
---

# Interpreter e2e: harness session boot (register-screen trap)

## Problem (bisected, verified)

First bad commit: **`2ea82b58f`** — `feat(auth-ui): force first-admin
registration before app access` (2026-10-14 10:05).

| Commit                             | Full interpreter suite   | Notes                                                                                                       |
| ---------------------------------- | ------------------------ | ----------------------------------------------------------------------------------------------------------- |
| `2dc4ca114` (parent, 10-14 00:44)  | **35 passed / 4 failed** | the 4 failures are the `user:`-given scenarios, failing fast (~1s, separate older defect; 3 later repaired) |
| `2ea82b58f` (guard, 10-14 10:05)   | **2 passed / 37 failed** | 33 error contexts are the register-screen trap                                                              |
| `331641244` (session start, 10-18) | 33 failed / 6 passed     | same deterministic trap set                                                                                 |

Mechanism (verified):

1. `backendLauncher.ts` boots each scenario with
   `PICASU_BOOTSTRAP_ADMIN=true` and an empty user store.
2. `given:` steps are API-side; on an empty store with the flag on,
   `POST /post/authenticate` succeeds in open mode **without creating a
   user** (`authenticate.rs` `open_mode`), so the store stays empty for
   the ~35 scenarios without a `user:` given.
3. The boot guard (originally `GET /get/users` probe, now
   `route/bootGuard.ts` + `GET /get/server-info`) sees empty+open on
   the first sessionless navigation and forces `/register`, so every
   UI assertion lands on the Create Admin screen.

Why it went unnoticed: precommit gates run vitest/clippy/eslint, not
Playwright — the introducing commit's own green was "Full frontend
suite (157)" (vitest). `just test` includes Playwright but runs
manually.

## Settled solution (band-aid; full redesign tabled separately)

The declaration-first redesign from the same discussion (pre-launch
givens, `picasu-admin` store provisioning, `server:` startup options,
`login:`/`index:` step verbs, `session: none` removal) is tabled in
`.plan/e2e-given-reshaping.md` (open). What follows is the interim
fix that keeps the suite green without that rework:

Product guard stays as-is (register-forcing is deliberate and pinned
by `bootGuard.test.ts`, the first-admin spec, and docs). The harness
gains a session boot:

- **Default**: after `given:`, the scenario boots into a real logged-in
  session: if the scenario declares no `user:`, the harness enrolls
  `admin` via `POST /post/users/bootstrap` (before `given:`, so
  `config:` given auth runs against a real user), then performs a real
  **UI login** (goto `/login`, fill, submit) before the first step.
  UI login over cookie/profile injection: couples to form selectors
  instead of storage-key internals, exercises the real login path, and
  feeds `covers.api: POST /post/authenticate` through the existing page
  request listener. `tracer.compare` only warns on missing coverage, so
  the extra API call is coverage-safe. One KDF round per scenario is
  negligible at the e2e `PICASU_PBKDF_ITERATIONS=1000`.
- **Explicit opt-out**: `session: none` in a scenario YAML — the
  harness does nothing; the scenario drives enrollment/login itself.
  For the scenarios that exercise the admin/enrollment/login aspect:
  `initial-onboarding`, `image-metadata-sidebar`,
  `admin-user-management`, `login-handling` (all four start with an
  explicit UI login step and declare `user:` given).

## Implementation steps

### H1 — DSL + session-boot module + opt-outs

1. `types.ts`: scenario-level `session: z.enum(['none']).optional()`
   (absent = harness-managed); keep `schema.json` in sync (hand-synced
   per `docs/playwright_generator.md`); document the key in the
   generator doc's Scenario DSL section.
2. New `tests/playwright/sessionBoot.ts`:
   - resolve session creds (first `user:` given entry, else
     `admin`/`ADMIN_PASSWORD` from `paths.ts`),
   - enroll via API before `executeGiven` when the scenario is
     harness-managed and has no `user:` given,
   - UI login after `executeGiven` (goto `/login`, fill User ID /
     Password, click Login, expect `/timeline`; throw with URL +
     page-state context on failure — a landing on `/register` means
     enrollment did not happen and must fail loudly).
3. Wire into `interpreter.spec.ts` between given and steps;
   `session: none` scenarios keep today's flow byte-for-byte.
4. Add `session: none` to the four self-managed scenarios (verify by
   grep that no other scenario contains login/register steps).

### H2 — `login-handling.yaml` independent red

Diagnose at HEAD after H1 (its failure predates the guard: it also
failed at `2dc4ca114`). Known suspect: `toast:error:unauthorized` is
stale against the settled no-snackbar 401 UX (`294355f7c`,
`docs/auth.md` "Session-denial UX", `login-session-banner.md` done) —
and `snapshot:login-page` may predate the static banner. Update
expectations to the settled contract (mirror of first-admin spec step
(6): stays on `/login`, no toast, banner visible), regenerate the
stored aria snapshot if stale. If diagnosis shows a real product gap
instead, stop and report — do not paper over it.

### H3 — full-suite gate (done)

`just frontend-playwright`: **42 passed / 0 failed** (exit 0) —
interpreter 39/39, siblings green; `just frontend-check` exit 0;
coverage-warning deltas from H1 were unmasking only (scenarios that
never reached `tracer.compare` while red). Long-term replacement:
`.plan/e2e-given-reshaping.md`.

## Verification

- Baseline numbers: pre-guard `35p/4f`, guard `2p/37f`, HEAD-of-work
  `33-34f`; target after: interpreter 39/39.
- Deterministic single-scenario probe: `-g "Filesystem watcher…"` passes.

## Notes

- `album-tree-refresh-after-move.spec.ts` is API-first with a bare
  `page.goto` smoke at the end; it is not part of the interpreter
  harness (it passed only because its assertions are API-side — the
  trailing gotos actually hit the register screen; out of scope here
  unless the H3 run shows it red).
- Related but distinct: `first-admin-registration.spec.ts`'s stale
  wrong-password snackbar expectation was fixed in
  `.plan/r26-remainder-bugset.md` (`51814fa09`).
