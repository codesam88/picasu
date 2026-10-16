---
status: open
type: chore
priority: high
area: backend
---

# Auth enrollment review — triaged view (2026-10-16)

Source claims: `.plan/auth-flows-enrollment.md` § Review claims 2026-10-15
(R1–R35, branch `auth-enrollment` @ `442162a0c`). Method: four parallel
Phase-1 reviews, then Phase-2 verification (three worker runs completed;
a fourth re-run was rate-limited, so the parent spot-checked the top
items against the tree instead). Verdicts below are the parent's: workers
provide evidence, the parent decides.

Severity: S0 broken/correctness, S1 security-high, S2 sec-med or
availability/docs-med, S3 cleanup/docs-low. Impact: HIGH changes APIs,
auth/token architecture, or module boundaries; MED changes data flow or
multi-file behavior; LOW is narrow lines, tests, or docs. Risk: chance
the fix causes behavior change or coverage loss.

## Priority order (high architecture/code impact first)

Ordered by severity, then HIGH impact first, discounted by HIGH fix risk.
This is the suggested tackle order, not a commitment to fix all items.

| Order | ID  | Verdict    | Sev | Impact | Risk    | Title                                                                    |
| ----- | --- | ---------- | --- | ------ | ------- | ------------------------------------------------------------------------ |
| 1     | R23 | CONFIRMED  | S1  | HIGH   | MED     | Auth state triplication + probe overload (root of R1)                    |
| 2     | R5  | CONFIRMED  | S1  | HIGH   | MED     | Session cookie not HttpOnly (client-set)                                 |
| 3     | R6  | CONFIRMED  | S2  | HIGH   | HIGH    | No server-side session revocation                                        |
| 4     | R26 | CONFIRMED  | S2  | HIGH   | MED     | Frontend auth layer fragmentation                                        |
| 5     | R3  | CONFIRMED  | S1  | MED    | MED     | Bootstrap race (check-then-act, no lock)                                 |
| 6     | R4  | CONFIRMED  | S1  | MED    | LOW     | Open registration squats first-user slot                                 |
| 7     | R1  | CONFIRMED  | S0  | LOW    | LOW     | Stale probe bounces post-login to /login                                 |
| 8     | R2  | CONFIRMED* | S0  | LOW    | LOW     | Backend tests don't compile (api_config; _api_watcher is runtime-only_)  |
| 9     | R7  | CONFIRMED  | S2  | MED    | MED     | No brute-force throttling                                                |
| 10    | R25 | PARTIAL    | S2  | MED    | MED     | Register-then-login non-atomic (retry yields 401, not 409)               |
| 11    | R24 | CONFIRMED  | S2  | MED    | LOW     | 401 interceptor double-toast + throttle-drop                             |
| 12    | R27 | PARTIAL    | S2  | MED    | LOW     | 422 catcher detail loss; /unauthorized doc false; bare-guard detail loss |
| 13    | R12 | CONFIRMED  | S2  | MED    | LOW     | Runtime bootstrap_admin flip → next-boot refusal                         |
| 14    | R13 | CONFIRMED  | S2  | MED    | LOW     | GET /get/config via GuardShare discloses paths                           |
| 15    | R8  | CONFIRMED  | S2  | LOW    | LOW     | GuardHashOriginal Forward masks denial as 200                            |
| 16    | R9  | CONFIRMED  | S2  | LOW    | LOW     | PUT bodies lack deny_unknown_fields                                      |
| 17    | R14 | CONFIRMED  | S2  | LOW    | LOW     | Unreachable register 401 + ?token= + phantom CLI refs                    |
| 18    | R16 | CONFIRMED  | S2  | LOW    | LOW     | auth-flows/auth/config docs describe deleted world                       |
| 19    | R19 | CONFIRMED  | S2  | LOW    | LOW     | rocket-guards brief points at deleted files/symbols                      |
| 20    | R21 | CONFIRMED  | S2  | LOW    | LOW     | Plan notes contradict code (3 items)                                     |
| 21    | R30 | PARTIAL    | S2  | MED    | HIGH    | Frontend 5→2 collapse (~−60 realistic, not −200)                         |
| 22    | R31 | PARTIAL    | S2  | LOW    | MED     | Source-regex tests (changePasswordForm is the only form check)           |
| 23    | R29 | CONFIRMED  | S3  | LOW    | MED     | Backend test dedup (keep per-struct admin-422; ~−110 not −160)           |
| 24    | R28 | CONFIRMED  | S3  | MED    | MED     | Config knob naming/read-back asymmetry                                   |
| 25    | R10 | CONFIRMED  | S3  | LOW    | LOW     | Closed-install UX vs plan flow A (plan or code must change)              |
| 26    | R15 | CONFIRMED  | S2  | LOW    | LOW     | HTTP-by-default exposes 14-day JWT (deploy note)                         |
| 27    | R11 | CONFIRMED  | S3  | LOW    | LOW     | 409 enumeration (plan-pinned, accepted risk)                             |
| 28    | R18 | CONFIRMED  | S3  | LOW    | LOW     | Playwright/scenario docs use deleted config.password                     |
| 29    | R17 | CONFIRMED  | S3  | LOW    | LOW     | openapi-reference stale (regen not rerun)                                |
| 30    | R20 | CONFIRMED  | S3  | LOW    | LOW     | frontend.md / test-strategy.md staleness                                 |
| 31    | R22 | CONFIRMED  | S3  | LOW    | LOW     | createUser caller-less + GivenUser.admin ignored (intentional-for-now)   |
| 32    | R32 | CONFIRMED  | S3  | LOW    | LOW     | Provision helper reuse (~−22)                                            |
| 33    | R33 | CONFIRMED  | S3  | LOW    | LOW     | Shared test-harness helpers (~−60)                                       |
| 34    | R34 | CONFIRMED  | S3  | LOW    | LOW-MED | Playwright API re-assertions (~−15)                                      |
| 35    | R35 | CONFIRMED  | S3  | MED    | MED     | Spawn/error boilerplate (~−50, optional, widest diff)                    |

## Verdicts with evidence (one line each)

- R1 CONFIRMED (S0/LOW/LOW). Only `RegisterPage.vue:150` calls
  `resetEnrollmentProbe`; `LoginPage.vue:99-127` replaces to `timeline`
  without reset, so a 401 cached at boot is re-read post-login
  (`enrollment.ts:58-70`, `routes.ts:126-141`). Fix: reset on
  login/logout or stop caching failures. Fixed 2026-10-16: probe and cache
  deleted, guard pings `GET /get/server-info` (`.plan/r26-frontend-auth-consolidation.md`).
- R2 CONFIRMED with correction (S0/LOW/LOW). `cargo check --tests` fails
  at `backend/tests/api_config.rs:125` (`no field password`); `api_watcher.rs`
  compiles and fails at runtime (bare-string → 422), not at compile time.
  Fix: drop legacy password assertions, send JSON bodies. Fixed 2026-10-16:
  bootstrap+JSON login via `Set-Cookie` session, removed `password`
  assertions (R1/R2 worker session).
- R3 CONFIRMED (S1/MED/MED). `bootstrap.rs:78-79` counts then
  `spawn_blocking` inserts with no lock; `provision_user`
  (`auth/users.rs:316-331`) re-checks id only; zero `BOOTSTRAP_LOCK` hits
  in `backend/src`. Fix: prove with concurrent test, guard check+insert
  (plan step 8).
- R4 CONFIRMED with nuance (S1/MED/LOW). The `user_count()>0` check at
  `register.rs:86` gates only the admin-session path; the knob/invite
  branch (`register_user_sync`) has no emptiness check, so the first user
  can land non-admin (pinned by `register.rs:91-118`). Fix: empty-store
  refusal on knob/invite branches.
- R5 CONFIRMED (S1/HIGH/MED). `sessionCookie.ts:28` sets `jwt` via
  `js-cookie` (cannot be HttpOnly); no server `Set-Cookie` in tree;
  `docs/auth.md:90` states the server never issues it. Fix: server-side
  issuance with HttpOnly + Secure + SameSite=Strict.
- R6 CONFIRMED (S2/HIGH/HIGH). `Claims` (`auth.rs:42-48`) has no
  version/jti; no logout route; `logout.ts:8` clears client-side only
  (demotion still re-checked per request). Fix: session epoch claim
  rotated on password change/logout.
- R7 CONFIRMED (S2/MED/MED). No rate-limit/lockout/429 symbols in
  `backend/`; PBKDF2-600k raises per-guess cost only. Fix: per-IP/identity
  counters with 429 on the four credential paths.
- R8 CONFIRMED (S2/LOW/LOW). `auth.rs:923-925` returns
  `Forward(Unauthorized)`; falls through to rank-11 SPA catch-all as 200,
  unlike `GuardHash` which returns `Error`. Fix: one-line change to
  `Outcome::Error(401)` + regression test.
- R9 CONFIRMED (S2/LOW/LOW). `deny_unknown_fields` present on
  register/bootstrap/authenticate but absent on `users_password.rs:14-20`
  and `users_admin.rs:12-17`; row-6 coverage exists only for the former
  three. Fix: add attribute to both PUT bodies + extend coverage.
- R10 CONFIRMED (S3/LOW/LOW). Plan demands locked screen naming
  `--bootstrap-admin`; code maps any probe failure to `login`
  (`enrollment.ts:12-17`, pinned by `enrollment.test.ts:32-34`). Fix:
  add `locked` destination or amend plan flow A + test case 2.
- R11 CONFIRMED (S3/LOW/LOW). `register.rs:128-136` and
  `invites.rs:334-339` return distinguishable 409; login stays uniform;
  reachable only with open knob/valid invite; plan-pinned. Fix: none —
  record as accepted risk.
- R12 CONFIRMED (S2/MED/LOW). `PUT /put/config` accepts `bootstrapAdmin`
  (`edit_config.rs:36,133-135`); `update()` persists with no gate
  (`config.rs:507-544`); gate runs only in `run()` (`lib.rs:100`).
  Fix: run freshness check in the PUT path and reject boot-breaking flips.
- R13 CONFIRMED (S2/MED/LOW). `get_config.rs:57-59` takes `GuardShare`
  and returns absolute `imagePath`, `uploadFolder`, address/port; already
  noted in `auth-hardening.md:125-126`. Fix: confirm intent; narrow share
  response or re-gate to `GuardAuth`.
- R14 CONFIRMED (S2/LOW/LOW). Register declares 401 (`register.rs:63`)
  but swallows guard failure (`:83-89` → 403/404/409); `extract_bearer_token`
  falls back to `?token=` (`auth.rs:446-451`, no frontend caller);
  `picasu-admin` cited in 4 messages but no such binary. Fix: drop query
  fallback, align spec or propagate, reword CLI remedy.
- R15 CONFIRMED (S2/LOW/LOW). No TLS in backend; cookie `secure`
  conditional with rationale; docs point at reverse proxy. Fix: docs
  deployment note only.
- R16 CONFIRMED (S2/LOW/LOW). `POST /post/users/create` in zero backend
  files yet documented in `auth.md:97`, `auth-flows.md:22,44,100,114`;
  bare-string/legacy-password text postdates removal commits. Fix: rewrite
  three docs against bootstrap/register flow.
- R17 CONFIRMED (S3/LOW/LOW). Reference documents `hasPassword` (spec has
  only `hasAuthKey`) and blank-clears-password text; regen recipe
  (`justfile:219-223`) not rerun since 2026-10-08. Fix: run
  `just docs-openapi`.
- R18 CONFIRMED (S3/LOW/LOW). `playwright_generator.md:142-143,195` and
  `scenario-dsl.md:147-156` show `config.password`; code has `givenUser`
  (`schema.json:71-85`, `executeGiven.ts:190-191`); scenarios migrated.
  Fix: replace examples with `user:` given.
- R19 CONFIRMED (S2/LOW/LOW). Brief cites deleted `post/users.rs` (`:113`)
  and `BOOTSTRAP_LOCK` (`:119`) plus legacy-migration rules. Fix: repoint
  at bootstrap/register or retire the brief.
- R20 CONFIRMED (S3/LOW/LOW). `frontend.md:103` omits `/register`
  (`routes.ts:17,31` has it); `:235` stale password wording; `e2e.rs`
  renamed to `api_*.rs`. Fix: refresh route list + wording + reference.
- R21 CONFIRMED (S2/LOW/LOW). Three contradictions: `auth-flows.md` row 1
  legacy-migrate `done` vs no migration in code; `create_user_handler`
  named vs deleted; pending-revert password-PUT entry vs already-applied
  (`users_password.rs:70-77` 404 + comment). Fix: fold applied reverts
  back into both plans.
- R22 CONFIRMED (S3/LOW/LOW). `createUser` has no `.vue` caller (register
  page uses `bootstrapUser`); `executeGiven.ts:191-198` states the admin
  flag is ignored. Fix: wire or delete when admin-create form lands.
- R23 CONFIRMED (S1/HIGH/MED). Cookie + module probe cache
  (`enrollment.ts:66`) + Pinia with no single owner; probe overloads
  admin `GET /get/users` with `validateStatus: () => true`, mapping every
  failure to `login`. Fix: one session module + explicit boot-status
  signal (revisit no-new-status-endpoint decision). Fixed 2026-10-16:
  session-module slice done — `GET /get/server-info` signal, collapsed
  guard, probe deleted, anonymous `GET /get/users` tied to 401
  (`.plan/r26-frontend-auth-consolidation.md`).
- R24 CONFIRMED (S2/MED/LOW). Interceptor (`axiosInterceptor.ts:101-104`)
  replaces all 401 messages then callers toast again; 1s throttle drops
  toast and redirect. Fix: single 401 display owner; scope throttle to
  redirects.
- R25 PARTIAL (S2/MED/MED). Two-step create→login with no rollback
  confirmed (`registration.ts:37-39`); retry-after-success yields 401
  (spec-pinned `:148-168`), not the claimed 409 — 409 needs a true race.
  Fix: return token from create round trip; re-scope race copy.
- R26 CONFIRMED (S2/HIGH/MED). `api/users.ts` vs `api/config.ts:31` vs
  six utils modules; login setup duplicated in two pages; users-scoped
  password route has no SPA caller. Fix: consolidate to one auth module.
- R27 PARTIAL (S2/MED/LOW). Fixed-string 422 (`catchers.rs:33-40`) and
  false `/unauthorized` no-body doc confirmed; "only bare-Status hits
  catchers" refuted — bare guard params also lose detail via codegen.
  Fix: explicit body parsing for 422 detail; route guards through
  `GuardResult`; fix doc.
- R28 CONFIRMED (S3/MED/MED). Three knob read styles; PUT accepts knobs
  GET never returns; frontend `AppConfig` stale both ways; plus
  stringly-typed status and `/post/invites/create` verb. Fix: knob
  accessor + `ConfigResponse` parity + typed helper + route alias.
- R29 CONFIRMED, estimate HIGH (S3/LOW/MED). Real dupes, but three
  `admin`-422 tests pin three different structs (one has live divergence
  per R9); realistic saving ~−110 not −160. Fix: keep one test per struct
  with shared setup.
- R30 PARTIAL, estimate HIGH (S2/MED/HIGH). Single call sites confirmed;
  −200 requires deleting 236 lines of tests with no component harness
  (`@vue/test-utils`/jsdom absent). Realistic ~−60 with re-homed tests.
  Fix: merge + keep suites as orchestration tests.
- R31 PARTIAL (S2/LOW/MED). `registerRoute` redundancy confirmed; but no
  e2e touches the change-password form, so `changePasswordForm.test.ts`
  is the only check of a plan step-5 requirement. Fix: delete only the
  redundant half; rewrite the form check as behavior test.
- R32 CONFIRMED, estimate plausible (S3/LOW/LOW). Non-invite branch
  duplicates `provision_user` order and error kinds (~−14); bootstrap
  delegate ~−5. Fix: call `provision_user(..., false)`; fold mapping.
- R33 CONFIRMED, estimate plausible (S3/LOW/LOW). Eight modules copy
  `lock()`/`setup()` (~−56); `authz::setup` needs a variant flag. Fix:
  one `tests/harness.rs`.
- R34 CONFIRMED, estimate plausible (S3/LOW/LOW-MED). Matrix already pins
  both API halves; keep UI halves + one status assertion per direction.
  Fix: drop two API round trips in the spec.
- R35 CONFIRMED, estimate mixed (S3/MED/MED). Join sites collapse
  (~−26…52); `db_err` saves characters not lines (all single-line);
  `?` vs `??` semantics must not merge. Fix: two helpers, per-crate,
  optional.

## What was checked by the parent (spot)

- HEAD `442162a0c`; `LoginPage.vue:99-127` no `resetEnrollmentProbe` vs
  `RegisterPage.vue:150` has it; `BOOTSTRAP_LOCK` zero hits in
  `backend/src`; `sessionCookie.ts:28` client-set, no server `Set-Cookie`;
  `bootstrap.rs:78-79` count-then-insert; `register.rs:86` vs `:124`
  gating asymmetry; `deny_unknown_fields` absent on the two PUT structs;
  `docs/auth.md:97` + `auth-flows.md:22,44,100` stale route references.

## Suggested first slices (if approved)

1. Correctness: R1 + R2 (unblock login proof and test suite).
2. Enrollment integrity: R4 + R3 (squat guard, then race lock/test).
3. Structural: R23 (+ R24/R26 narrowed to the session module), not the
   full fragmentation cleanup at once.
4. Small sec fixes: R8 + R9 + R12 (one-line to few-line each).
5. Plans + docs: R21 + R16/R19 (fold reverts back before more doc edits).

No fixes applied in this change; no behavior changed.

## Follow-up

Sequence findings (R1, R3–R4, R23–R26) outgrew per-case tests and are
planned as model-based flow conformance in `.plan/flow-model-conformance.md`
(single golden model, backend-first then e2e drivers).
