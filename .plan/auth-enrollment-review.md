---
status: done
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

## Priority order (live items; refreshed 2026-10-20)

The original 2026-10-15 table listed every finding and is preserved in
git history. Below is the live set — everything else is fixed, struck,
or discharged (per-verdict annotations in the verdict lines; fixed
R1–R9, R12–R28; discharged to owning plans: R10, R22, R25, R30,
R31; struck as churn/no benefit: R29, R34, R35; R11+R32 closed
`f677f67f1`).

**Live set is empty.** R14 closed 2026-10-20: the `picasu-cmd` CLI now
exists (`.plan/user-store-redb.md`), so the references the findings
flagged point at a real tool. The remaining e2e-harness adoption is
tracked by `.plan/e2e-given-reshaping.md` (H1b–H1e), not here.

| Order | ID  | Sev | Impact | Risk | Remaining work |
| ----- | --- | --- | ------ | ---- | -------------- |
| —     | —   | —   | —      | —    | none           |

Discharged 2026-10-19 — rows removed from the tackle order, analysis
folded into the verdict lines below: R10, R22, R25 → owning plans
(`flow-model-conformance.md`, `auth-admin-panel.md`); R30, R31 →
`.plan/frontend-vue-consolidation.md` T1/T2; R29, R34, R35 struck as
churn/no benefit.

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
  (plan step 8). Fixed 2026-10-16: `BOOTSTRAP_LOCK` serializes the
  authoritative flag ∧ empty-store check with the insert inside
  `bootstrap_user_sync`; concurrent-test pins exactly-one-winner.
- R4 CONFIRMED with nuance (S1/MED/LOW). The `user_count()>0` check at
  `register.rs:86` gates only the admin-session path; the knob/invite
  branch (`register_user_sync`) has no emptiness check, so the first user
  can land non-admin (pinned by `register.rs:91-118`). Fix: empty-store
  refusal on knob/invite branches. Fixed 2026-10-16: `register_user_sync`
  refuses 401 on an empty store after validation, before gates (invite not
  burned); knob/invite matrices reseeded onto non-empty stores.
- R5 CONFIRMED (S1/HIGH/MED). `sessionCookie.ts:28` sets `jwt` via
  `js-cookie` (cannot be HttpOnly); no server `Set-Cookie` in tree;
  `docs/auth.md:90` states the server never issues it. Fix: server-side
  issuance with HttpOnly + Secure + SameSite=Strict. Fixed 2026-10-16:
  server issues HttpOnly `Set-Cookie` (`authenticate.rs:50-58`),
  `sessionCookie.ts` deleted, `.plan/r5-httponly-session-cookie.md` done.
- R6 CONFIRMED (S2/HIGH/HIGH). `Claims` (`auth.rs:42-48`) has no
  version/jti; no logout route; `logout.ts:8` clears client-side only
  (demotion still re-checked per request). Fix: session epoch claim
  rotated on password change/logout. Fixed 2026-10-16: `Claims` carries
  `jti` with server-side session inventory, `POST /post/logout` revokes,
  password change clears sessions (`.plan/r6-session-revocation.md` done).
  Re-shaped 2026-10-22 (`.plan/auth-state-model.md`): the inventory is
  gone — the token carries the user's token version (`ver`), the guard
  checks it against the identity record, and the server keeps only a
  revocation list (a logged-out `jti`, until it expires). Password and
  role changes bump `ver` in the same transaction as the change.
- R7 CONFIRMED (S2/MED/MED). No rate-limit/lockout/429 symbols in
  `backend/`; PBKDF2-600k raises per-guess cost only. Fix: per-IP/identity
  counters with 429 on the four credential paths.
- R8 CONFIRMED (S2/LOW/LOW). `auth.rs:923-925` returns
  `Forward(Unauthorized)`; falls through to rank-11 SPA catch-all as 200,
  unlike `GuardHash` which returns `Error`. Fix: one-line change to
  `Outcome::Error(401)` + regression test. Fixed 2026-10-16: denial is now
  `Error(401)`; `original_serving_without_allow_original_is_401` pins it.
- R9 CONFIRMED (S2/LOW/LOW). `deny_unknown_fields` present on
  register/bootstrap/authenticate but absent on `users_password.rs:14-20`
  and `users_admin.rs:12-17`; row-6 coverage exists only for the former
  three. Fix: add attribute to both PUT bodies + extend coverage.
  Fixed 2026-10-16: attribute added to both bodies with one 422 test each.
- R10 → carried by `.plan/flow-model-conformance.md`. Discharged
  2026-10-19. The stale-probe mechanism cited at report time is gone
  (R1/R23 fix — probe deleted; the closed-install decision now lives in
  `route/bootGuard.ts` behavior tests). Residual = the flow A
  locked-screen demand vs the current boot-to-login mapping; the Layer-2
  model task encodes flows A–C and forces the plan-vs-code
  reconciliation, so the decision lands there rather than as an
  isolated change.
- R11 REOPENED (S3/LOW/LOW). `users_register.rs` and
  `invites.rs` (two sites) return distinguishable 409 bodies; both
  echo the probed id. Reachable on the invite path without any session.
  Owner decision 2026-10-19: not an accepted risk. Fixed 2026-10-20
  (`f677f67f1`): every surface emits the shared
  `REGISTRATION_CONFLICT_MESSAGE` ("the invite or the userId is already
  taken"), no id echo; byte-identical bodies pinned by
  `registration_conflict_bodies_are_indistinguishable`.
- R12 CONFIRMED (S2/MED/LOW). `PUT /put/config` accepts `bootstrapAdmin`
  (`edit_config.rs:36,133-135`); `update()` persists with no gate
  (`config.rs:507-544`); gate runs only in `run()` (`lib.rs:100`).
  Fix: run freshness check in the PUT path and reject boot-breaking flips.
  Fixed 2026-10-16 (narrowed per owner decision): `bootstrapAdmin` is
  host-only — any presence in `PUT /put/config` is a 400 naming the
  flag/env/file setters, and the assignment is deleted. Extended the same
  day: `address`, `port`, `allowRegistration`, `trivialPasswords`
  likewise host-only (per-key 400 + setters, assignments deleted,
  `docs/config.md` documents the API-vs-host split). The remaining
  knob-matrix work is folded into that section.
- R13 CONFIRMED (S2/MED/LOW). `get_config.rs` takes `GuardShare` and
  returned bind `address`/`port` and absolute `imagePath`/
  `uploadFolder` to any share holder; noted in
  `auth-hardening.md:125-126`. Owner decision 2026-10-20 (option B+C).
  Fixed 2026-10-20 (`69d37ed37`): `address`/`port` deleted from
  `ConfigResponse` (no SPA consumer), share-credential callers get
  `imagePath`/`uploadFolder` as `null`, admin tier keeps the real
  values; pin `share_tier_config_hides_host_fields`. Discovered
  constraint: `GuardShare` admits admins and shares only — non-admin
  sessions are refused 401 before the handler, so the hide applies to
  share claims alone and the tier split needs no user-tier distinction.
- R14 CONFIRMED (S2/LOW/LOW). Register declares 401 but swallows guard
  failure (→ 403/404/409 at the handler); `extract_bearer_token` fell
  back to `?token=`; `picasu-admin` cited in messages but no such
  binary. Partial 2026-10-19: the `?token=` fallback is gone. Converged
  2026-10-20: the 401 annotation is backed by the reachable empty-store
  refusal in `register_user_sync` (401 `not_enrolled_message`,
  added with the open-mode removal); the guard-`Err` swallow is owned
  by R27's `GuardResult` rollout; the CLI references are now true —
  the `picasu-cmd` binary is delivered (`.plan/user-store-redb.md`;
  the reference rename shipped with S3). Nothing left to fix in R14 itself.
  Closed 2026-10-20: delivered `picasu-cmd` satisfies the reference
  obligation; harness adoption is H1b–H1e of `.plan/e2e-given-reshaping.md`.
- R15 CONFIRMED (S2/LOW/LOW). No TLS in backend; cookie `secure`
  conditional with rationale; docs point at reverse proxy. Fix: docs
  deployment note only. Fixed 2026-10-19: HTTP + 14-day-JWT deployment
  note in `docs/config.md` (TLS at the proxy, `X-Forwarded-Proto` /
  `trust_proxy_forwarded_for` forwarding).
- R16 CONFIRMED (S2/LOW/LOW). `POST /post/users/create` in zero backend
  files yet documented in `auth.md:97`, `auth-flows.md:22,44,100,114`;
  bare-string/legacy-password text postdates removal commits. Fix: rewrite
  three docs against bootstrap/register flow. Fixed 2026-10-19:
  `docs/auth-flows.md` + `docs/auth.md` rewritten against the
  bootstrap/register reality; zero `users/create` hits in `docs/`.
- R17 CONFIRMED (S3/LOW/LOW). Reference documents `hasPassword` (spec has
  only `hasAuthKey`) and blank-clears-password text; regen recipe
  (`justfile:219-223`) not rerun since 2026-10-08. Fix: run
  `just docs-openapi`. Fixed 2026-10-16: reference and `backend/openapi.json`
  regenerated (`c89fc1cea`), zero `hasPassword` hits.
- R18 CONFIRMED (S3/LOW/LOW). `playwright_generator.md:142-143,195` and
  `scenario-dsl.md:147-156` show `config.password`; code has `givenUser`
  (`schema.json:71-85`, `executeGiven.ts:190-191`); scenarios migrated.
  Fix: replace examples with `user:` given. Fixed 2026-10-19: both docs
  carry the `user:` given form and the current config-given fields.
- R19 CONFIRMED (S2/LOW/LOW). Brief cites deleted `post/users.rs` (`:113`)
  and `BOOTSTRAP_LOCK` (`:119`) plus legacy-migration rules. Fix: repoint
  at bootstrap/register or retire the brief. Fixed 2026-10-19, genre
  correction: `docs/briefs/*.md` are upstream-knowledge digests, so the
  brief was rewritten project-independent against the Rocket 0.5.1
  `FromRequest` docs (Outcome trio, `Option`/`Result` catch impls,
  guard ordering, request-local caching) with upstream-only sources —
  project churn can no longer stale it.
- R20 CONFIRMED (S3/LOW/LOW). `frontend.md:103` omits `/register`
  (`routes.ts:17,31` has it); `:235` stale password wording; `e2e.rs`
  renamed to `api_*.rs`. Fix: refresh route list + wording + reference.
  Fixed 2026-10-19: `/register` + boot-guard routing + current password
  wording in `docs/frontend.md`; `e2e.rs`/gap staleness cleared in
  `docs/test-strategy.md`.
- R21 CONFIRMED (S2/LOW/LOW). Three contradictions: `auth-flows.md` row 1
  legacy-migrate `done` vs no migration in code; `create_user_handler`
  named vs deleted; pending-revert password-PUT entry vs already-applied
  (`users_password.rs:70-77` 404 + comment). Fix: fold applied reverts
  back into both plans. Fixed 2026-10-19: the two plan rows/notes folded
  to the proven state, the enrollment plan's pending-revert entries moved
  to applied with pin names, `create_user_handler` references cleared —
  and the tracker file (`.plan/auth-flows.md`) subsequently removed as
  redundant (`f1877ed7e`); per-flow state lives in the plan files.
- R22 → carried by `.plan/auth-admin-panel.md` (step 2: the
  admin-create form wires `createUser` to `POST /post/users/register`
  or deletes it — no dead export remains either way). Discharged
  2026-10-19.
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
  redirects. Fixed 2026-10-17: the interceptor became the single 401
  display owner (`294355f7c`), the caller-side sweep completed
  (`4b6c23aba`), and the 1s throttle was deleted as a consumer of it
  (`a42e7f2af`).
- R25 → carried by `.plan/flow-model-conformance.md`. Discharged
  2026-10-19. The two-step create→login split (with no rollback;
  retry-after-success yields 401 per the spec pin) was confirmed, but
  the original fix text ("return token from create") predates the R5
  HttpOnly change — the post-cookie decision is
  register-issues-session vs keep the two-step, which the Layer-1/2
  model task forces when it encodes the enrollment flow; the
  per-case sequencing tests are absorbed by the conformance drivers
  rather than duplicated.
- R26 CONFIRMED (S2/HIGH/MED). `api/users.ts` vs `api/config.ts:31` vs
  six utils modules; login setup duplicated in two pages; users-scoped
  password route has no SPA caller. Fix: consolidate to one auth module.
  Re-triaged 2026-10-18 against HEAD `331641244` (two-round critical
  review): the password-route sub-point is stale (`ChangePassword.vue:114`
  calls it via `setUserPassword`); the utils/auth modules no longer send
  HTTP; the "one auth module" merge is deferred by the session-slice
  scoping decision (`r26-frontend-auth-consolidation.md`), not a pending
  fix — `api/users.ts` already is that module. Verified remainder is a
  bug set (failed-login 401 clears a live display session; share-create
  double snackbar; guard behavior-coverage hole; `AUTHENTICATE_URL`
  literal ×3; phantom `/get/get-all-shares` call) plus an S3 ride-along
  tail (ceremony dup, orphaned config bindings, stale comments/docs) ->
  `frontend-vue-consolidation.md`. Superseded 2026-10-18 by
  `.plan/r26-remainder-bugset.md`, which owns fixes from here.
  Executed 2026-10-18 → done (`7728075d4`…`c8d4147f1`): O2 backend
  401 retype (privilege → 403, value-credential → 400, 401 left
  session-terminal; `GuardResult` keeps reason-bearing bodies),
  interceptor authenticate carve-out + single `AUTHENTICATE_URL`
  source, share-create single display owner, boot-guard decision →
  `route/bootGuard.ts` behavior tests with ping-rejection degrade,
  phantom shareStore members deleted. Caveat: `interpreter.spec.ts`
  scenario red predates execution (baseline `2ee4ea835`), tracked in
  `.plan/e2e-boot-guard-scenario-red.md`.
- R27 PARTIAL (S2/MED/LOW). Fixed-string 422 (`catchers.rs:33-40`) and
  false `/unauthorized` no-body doc confirmed; "only bare-Status hits
  catchers" refuted — bare guard params also lose detail via codegen.
  Fix: explicit body parsing for 422 detail; route guards through
  `GuardResult`; fix doc. Re-verdicted 2026-10-19: the fixed-string 422
  and the 401 JSON body are now the documented contract
  (`4fd98860a`), and `GuardResult` propagation (established while
  retyping `users_password`) is the bare-guard-detail rollout vehicle —
  residual is that rollout plus any remaining bare-guard routes, not a
  catcher change. Executed 2026-10-20 (`bcf65697e`): all ten bare
  `GuardAuth` holders flipped (`GuardResult<GuardAuth>` + `let _ =
auth?;`); denial bodies now carry the guard's real `AppError`
  (window-aware enrollment hint). The enroll_gate Tie-off body
  assertion, dropped when the swallow forced bodyless pins, is
  restored. Sweep residue: `TimestampGuardModified` stays bare by
  construction (Forward-selected presenter guard, `()` error type —
  `GuardResult` cannot express it); no other bare-guard sites. Statuses
  unchanged; catcher remains for framework-level rejections.
- R28 CONFIRMED, residual (S3/MED/MED). Re-scoped 2026-10-19: the
  `ConfigResponse` read-back/parity half is owned by
  `.plan/auth-admin-panel.md` step 1, and the R12 host-only split
  already redefined what API-vs-host parity means. Fixed 2026-10-20
  (`4551928f2`), owner decisions taken per residual: (1) typed knob
  accessor — all non-test `APP_CONFIG` read sites now go through a
  single `with_config()` closure helper in `model/config.rs`
  (behavior-neutral, no red); (2) verb alias — `POST /post/invites/create`
  renamed to `POST /post/invites`, hard removal without a deprecation
  window (zero SPA consumers verified; the invite UI lives in the
  unbuilt admin panel) with a dated oasdiff entry, operationId keeps
  `create_invite_handler`; (3) stringly-typed status — dropped from
  wire, contract, and `serverErrorSchema`: `ErrorStatus` was dead
  end-to-end (never read anywhere; `Temporary` ⟺ `RateLimited` ⟺ 429 +
  `retryAfter`, so retryability was already encoded by `kind` → HTTP
  status plus `Retry-After`), introduced pre-review in `c6e599a95`
  without an owner decision. New pins: invites noun-path route accepts
  admin creates; error bodies carry no `status` key.
- R29 STRUCK 2026-10-19 (churn). Re-verified: the named 422 tests are
  one-per-struct (tracker policy), the four bare-string tests are
  distinct store-state matrix rows — there is no duplicate to delete.
  Real residue: a ~7-line bare-string block embedded in
  `no_legacy_gate_flag_controls_empty_store` and a one-line comment
  drift (`tests/auth_users.rs` says bare-string "is 400"; the cited
  tests assert 422) — the comment fix rides along with R33. The ~−110
  estimate was harness-copy mass that belongs to R33 instead.
- R30 → carried by `.plan/frontend-vue-consolidation.md` T1
  (~−60…−80 / ~2 units). Discharged 2026-10-19. Correction folded in:
  "single call sites" was wrong for 2 of 5 modules (`currentUser` ×4,
  `retryAfter` ×2); the merge saves mostly file/import ceremony and
  requires re-homing 5 test suites with no functional benefit —
  ride-along only when those modules are next touched.
- R31 → carried by `.plan/frontend-vue-consolidation.md` T2.
  Discharged 2026-10-19. Correction folded in: only S9 (~−7) is freely
  deletable — S7/S8 are the only routes-side wiring pins, B3 keeps the
  rest as tripwires, and the change-password form keeps zero e2e
  contact (coverage fact, not a deletion). Change-password "rewrite as
  behavior test" adds code without adding coverage: churn.
- R32 ride-along (S3/LOW/LOW). Executed 2026-10-20 inside the R11 fix
  (`f677f67f1`): the gated register branch delegates to
  `users::provision_user`; the invite path keeps `redeem_invite`
  (atomic mark-insert). Same file states as `bootstrap_user_sync`;
  empty-store refusal ordering untouched; `create_user`'s defensive
  duplicate bail stays (unreachable through the pre-check).
- R33 CONFIRMED, estimate plausible (S3/LOW/LOW). Eight modules copy
  `lock()`/`setup()` (~−56); `authz::setup` needs a variant flag. Fix:
  one `tests/harness.rs`. Re-verified 2026-10-19: census is now
  15×`lock()` + 15×`setup()` (2 image-home variants: `authz.rs`,
  `auth_users.rs`), 9 cookie-family helpers, 2×`set_bootstrap_admin`,
  3×`login()` return shapes, 3 seed helpers — net ~−180…−220, safe to
  centralize (TEST_SERIAL_GUARD is a process-global static, TEST_ENV is
  `LazyLock` — no double-init). Cheapest form: `pub fn
lock/setup[_image_home]/cookie(id, admin)` in `tests/bootstrap.rs`,
  no new harness.rs. Highest-RoI item of the re-verification pass;
  stands alone (details: § Re-verification 2026-10-19). Executed
  2026-10-19 (`fb34594dd`): 16 files, +300/−546 (net −246) — helpers are
  pub fns in `tests/bootstrap.rs`; enroll_gate aliases
  `login_status as login`; r6 logins keep strict-Ok asserts at adapted
  call sites; `auth/invites.rs` untouched (different `setup` semantics);
  R29's comment drift (`tests/auth_users.rs` "400"→422) folded in.
  Gates: cargo test 727 passed / 0 failed (fresh), fmt + clippy clean,
  no test deleted or renamed.
- R34 STRUCK 2026-10-19 (churn). Steps 2 and 3 proved load-bearing
  counter-proofs, not re-assertions: step 2's probe is the
  creates-nothing proof for the mismatch path, and step 3's
  `GET /get/users` round trip is the only e2e proof that a UI submit
  persisted `admin:true` (the `given:` provisioning writes the store
  directly, bypassing the UI). Only step-9's closed-window 401
  (~−4…−8 lines) is trimmable — not worth a slice.
- R35 STRUCK 2026-10-19 (no benefit). All 24 join sites are already
  single logical lines — a `run_blocking` helper is a ~30-chars/site
  readability aid with ≈0 line delta, as are the 67 narrow / 87
  all-form `map_err(Database)` sites. The `??` caveat is real for eager
  call-operand evaluation but harmless at the two named pure sites.
  Two units of diff churn across 19 files for nothing — do not
  schedule.

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

Retriage 2026-10-16 (branch `auth-enrollment-review` @ `c3d0822`, five
read-only batch checks + spot-checks): resolved R1, R2, R5, R6, R17, R23
(session slice; R26 remains partial, session module only).
Re-triage 2026-10-19 (post re-verification, churn/absorption pass):
live items = R11 (re-opened, not accepted risk), R13, R14 (residual:
register-401 annotation), R27 (residual `GuardResult` rollout), R28
(residual: typed helper + verb alias + status). Done: R33 2026-10-19
(`fb34594dd`, net −246 lines, incl. R29's comment residue). Ride-along
inside the R11 touch: R32. R11+R32 done 2026-10-20 (`f677f67f1`).
R13 decided 2026-10-20 (option B+C, owner-approved) and fixed the same
day (`69d37ed37`). R27 residual executed 2026-10-20 (`bcf65697e`) —
all bare `GuardAuth` guards propagate detail; the 422/401 catcher
contract and `TimestampGuardModified` stand as documented. R28 done
2026-10-20 (`4551928f2` — knob accessor, invites verb rename, wire
status drop). R14 done 2026-10-20 — the `picasu-cmd` CLI is delivered,
so the cited tool exists. Live set empty; harness
adoption continues in `.plan/e2e-given-reshaping.md`. Closed 2026-10-23:
all verdicts resolved and the last deferred slice (password-PUT
throttling, PR #133) landed; remaining open work (share-password
hardening, login-throttle polish) is owned by `.plan/auth-hardening.md`.

Decision 2026-10-19 (open-mode removal, owner-approved this session):
the `ephemeral-open-mode` structure is deleted, not just the mint.
Removed: `POST /post/authenticate`'s no-credential arm
(`authenticate.rs:363-370`, minted `Claims::new_user("admin", true)`
for any parseable body, throttle-skipped) and the guard arms returning
an ephemeral admin identity in-window (`auth.rs:605-609` and the second
family at `:903-908`). Rationale: no consumer (the UI pre-enrollment
touches only `server-info` + `bootstrapUser`; the guard already opened
admin access cookie-less, making the mint redundant), the explicit-act
principle (through the HTTP API, bootstrap is the only first act on an
empty store — CLI provisioning edits the store offline), and a uniform
source-of-truth rule instead of per-route conditionals. Resulting
semantics: `flag ∧ empty` exposes exactly `GET /get/server-info` and
`POST /post/users/bootstrap`; every guarded route and `authenticate`
answer 401 "not enrolled" (authenticate's case inherits the unknown-
user path with its uniform dummy-KDF cost; the throttle now applies
in-window too). Status stays uniform per the O2 retype; the _message_
branches on window state (open → "enroll via
`POST /post/users/bootstrap`"; closed → the existing
`--bootstrap-admin`/CLI wording, `enroll_gate.rs:390` pin intact).
`POST /post/users/bootstrap` remains anonymous by design — still
bootstrap-only enrollment. Fold-outs: R14 residual (register-401
annotation wording) unchanged; R10 unchanged; R11 unchanged. Updates in
this change: flow-model plan drops the `ephemeral-open-mode` state;
`docs/auth.md`/`auth-flows.md` open-mode text; OpenAPI auth
descriptions + regen. Discharged to owning plans: R10, R25 →
`.plan/flow-model-conformance.md`; R22 → `.plan/auth-admin-panel.md`;
R30, R31 → `.plan/frontend-vue-consolidation.md` (T1/T2). Struck as
churn/no benefit: R29, R34, R35. Closed since that triage: R3, R4,
R7–R9, R12, R15–R16, R18–R21, R24, R26 — per-verdict annotations above.
In progress: R4+R3 done 2026-10-16, R8+R9+R12 done 2026-10-16, host-only
knob extension + `secrets.password` doc done, auth-key secrets-file slice
done 2026-10-16 (`.plan/auth-key-secrets-file.md`), then R26 remainder.
R26 remainder re-scoped then executed 2026-10-18 — now done in
`.plan/r26-remainder-bugset.md`; S3 ride-along tail parked in
`.plan/frontend-vue-consolidation.md`.

Sequence findings (R1, R3–R4, R23–R26) outgrew per-case tests and are
planned as model-based flow conformance in `.plan/flow-model-conformance.md`
(single golden model, backend-first then e2e drivers).

R7 done 2026-10-17 (`.plan/r7-bruteforce-throttling.md`): three buckets
on `POST /post/authenticate` (IP-short 3/10min→10s cap 8h, IP-long
16/24h→30min cap 8h, user-long 18/24h→30min cap 2h; two failures share
each level), pre-KDF 429 + `Retry-After`, per-claimed-name keying with no
existence oracle, 13 host-only `[server]` knobs + env overrides,
`PICASU_RATE_LIMITS=false` env-only kill switch, HttpOnly-safe login UI
(countdown banner, interceptor silent on 429), live e2e to ban and back.
Accepted risks: restart-clear in-memory state, single-node only,
proxy-header trust opt-in, top-18 password burst residual (password
policy carries it).

Fix batch 2026-10-17 (four-lens review triage): reservation slots close
the check→KDF→record TOCTOU (parallel bursts cannot multiply the
threshold; serial behavior unchanged); success keeps IP-long history
(account holders cannot wipe a shared IP's daily budget); kill-switch
tests use a forced flag (no environment mutation); 429 assertions pin
header↔body coherence within wall-clock skew; `.temporary()` on
throttle errors; shared `AppConfig` schedules helper; tightened
visibility; shared frontend retry string; handler tests for user-long /
IP-long wiring, proxy-header trust on/off, and sweep cadence; log
`user_id` sanitized; over-cap eviction spares live bans and held
reservations. Deliberately deferred: IPv6 /64 keying, trusted-proxy
CIDR list, log sampling, openapi 429 body schema, named-config struct,
share-password hardening, password-PUT throttling — done 2026-10-23
(PR #133: dedicated `(ip, jti)` instance, 3-per-24h budgets,
revoke-on-overstep + 401, attempts-left 400s).

## Re-verification 2026-10-19 (R29–R35 critical review, standard tier)

Two-pass (discover + challenge) re-check against HEAD; R33 census and
the consolidation parking point spot-checked by the parent:

- R29 DOWNGRADED: the three-to-five named 422 tests are one-per-struct
  (tracker policy), the four bare-string tests are distinct store-state
  rows — only the ~7-line bare-string block embedded in
  `no_legacy_gate_flag_controls_empty_store` is a real row overlap.
  Real content −1…−8 (plus one comment-line drift:
  `tests/auth_users.rs` says bare-string "is 400", tests assert 422).
  No standalone slice; residue folds into R33.
- R30 accepted, −60…−80 as estimated, but the "single call sites" rate
  was wrong for 2 of 5 modules (`currentUser` ×4, `retryAfter` ×2);
  merger still saves ~−20…−40 src with suites re-homed. Harness
  (@vue/test-utils/jsdom) still absent. Now owned as T1 in
  `.plan/frontend-vue-consolidation.md`.
- R31 DOWNGRADED: S7–S10 tripwires verified, but only S9 (~−7) is
  freely deletable — the rest is load-bearing wiring pins or blocked by
  the r26-remainder B3 keep decision. Now owned as T2 there.
- R32 accepted, corrected −14…−18 (bootstrap half already a delegate);
  delegation verified ordering-safe (R4 refusal precedes the gated
  branch) and pin-free (no test greps the message string). Ride-along
  in any register touch.
- R33 DOWNGRADED upward: census is 15×`lock()`, 15×`setup()`
  (2 image-home variants), 9×`cookie()`-family, 2×`set_bootstrap_admin`,
  3×`login()` shape variants, 3 seed helpers — net ~−180…−220, safe
  (TEST_SERIAL_GUARD is a process-global static; TEST_ENV is LazyLock;
  no per-module init to double-run). Cheapest realization: add
  `pub fn lock/setup[_image_home]/cookie(id, admin)` to
  `tests/bootstrap.rs` directly — no new harness.rs. Highest RoI of the
  set; the only standalone-slice-worthy item.
- R34 DOWNGRADED: only the step-9 closed-window 401 (~−4…−8) is
  trimmable; steps 2 and 3 assertions are load-bearing counter-proofs
  (step 3 is the only e2e proof a UI submit persisted admin:true;
  `executeGiven` provisions DB directly). Do not cut step 3.
- R35 DOWNGRADED: line savings ≈ 0 — all 24 join sites and the 67
  narrow / 87 all-form `map_err(Database)` sites are already single
  logical lines; a `run_blocking`/`db_from` helper is a readability
  aid (~30 chars/site), not a reduction. The `??` caveat is real for
  call-operand eagerness but does not change behavior at the two named
  pure sites. Optional polish only; do not schedule for lines.
- Miss-hunt M1: the S3 ride-along tail pointers into
  `.plan/frontend-vue-consolidation.md` were dangling (none of the
  promised items were present) — fixed by parking the tail there
  (T1–T3) alongside this note.
