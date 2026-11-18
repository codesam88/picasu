---
status: open
type: feature
priority: medium
area: testing
---

## Problem

The Quint catalogs and generated docs (goals, assumptions, scope,
requirements, events) are in place, and `quint run` executes scenario runs at
10k samples. Sampled execution gives confidence, not verification: nothing
exhaustively checks the invariants over all reachable states within the
modeled bounds.

Correction to the earlier framing: the shared prerequisite largely exists.
`eventual-consistency.qnt` already defines `action init` (state module) and
`action step = any { ... }` (~100 argument-enumerated branches). The gaps:
the 16 asset-aspects actions are in no step; no `Skip`/stutter branch; no
`Settled`-conditional invariant form; no branch-completeness enforcement;
no scope entry for scheduler nondeterminism; nothing wired to
`quint verify`.

## Goal

A nightly proof lane that model-checks the Quint models with `quint verify`,
on the asset-aspects slice first, with backend choice decided by measured
data rather than assumption. Safety-only: settled-state invariants, no
liveness/fairness (ASS-6, SCOPE-3). Bounded model checking is the intended
stop; inductive invariants only if the data shows the unbounded claim is
actually needed.

Validation framing (user-confirmed): the environment is the set of external
actors — filesystem events, API calls, faults — which may fire at any time;
the system under proof is the pipeline machinery (intake, worker, sweep,
aspect-table maintenance). The goal is eventual consistency in the
as-if-serial sense: once the pipeline drains, the DB/derived state agrees
with the FS record (the durable serial log of user actions, ASS-3/G5), all
modifications applied in generation order, none lost.

## Implementation steps

1. Extend the step: new module `utils/quint/pipeline/asset-aspects-step.qnt`
   importing the aspects + base modules; `action stepAspects = any { ... }`
   re-enumerating the base branches plus the 14 aspects actions over their
   parameter ranges, plus a `Skip` (all-vars stutter) branch. Distinct name
   avoids shadowing the inherited `step`. Coarse and fine-grained action
   variants coexist deliberately (both code-faithful at different
   resolutions; the fine ones expose the lease/torn-window intermediate
   states). The base specs and all 33 runs stay untouched.
2. Invariant re-expression — resolved by the phased completion machine
   (replaces the plain `Settled implies Converged` approach, which a first
   Apalache run falsified with a documented ghost shape):
   - Phases in the step module (base modules untouched; `--init initCompletion`
     wrapper): 0 free (any action + BeginDrain) → 1 draining (drain-set +
     ProcessCrash/LoseThumbs/DbLoss + untracked ScanRoot) → 2 scanning
     (tracked `CompleteScan(r)` per root, mandatory, then resolution of what
     scans create; fault-free — the one uninterrupted cycle).
   - `CompletionInvariant = (phase = 2 ∧ scannedRoots = ROOTS ∧ Quiescent)
⇒ (Converged ∧ AspectsAligned)`.
   - Ordering rationale (user-confirmed): scan triggers mix with events,
     API calls and crashes in phases 0–1; only the final cycle is assumed
     uninterrupted. `ScanRoot` creates work (submits intake, sets
     sweepPending unconditionally), so the scan is last and its findings
     drain inside phase 2 — G2's "first backstop cycle that starts" shape.
   - Crash resilience: crashes destroy pending work (ProcessCrash clears
     intake/batch/leases; DbLoss clears the DB projection); whatever the
     drain loses, the final scan cycle restores. Crash-heal stays covered
     because completion can be entered from any reachable state.
   - `Quiescent` includes ∀r: not sweepPending(r) (a set flag is pending
     work — SweepRoot is enabled whenever it holds).
   - Known caveat: if quiescence is unreachable from some phase-1 state
     (incomplete drain set, e.g. an uncompletable lease), its traces
     silently never reach a checked state — review the drain-set table
     for exhaustiveness.
     Status: implementation landed (fdfc816a): one `stepCompletion` action
     (1507 branches, phase-guarded via `Stay` wrappers for var-update-set
     uniformity), `Quiescent` with the sweepPending conjunct, checker
     extended to exempt `init*` entry actions (selftest 17). `stepAspects`
     superseded and removed. Drain-set reclassifications: `RemoveIdOnly`
     is crash-window residue (phases 0+1; its only repair is a scan);
     `IntakePickupFileGone` is a consumer; `MemberWrite` is a pure
     stutter. Measurement note: uniform sampling essentially never
     reaches phase 2 (~250 enabled branches vs a ~12-branch completion
     path), so `quint run` sampling is weak for this invariant — Apalache
     BMC is the operative check (a violating trace at 10k samples/30
     steps is found in seconds).
     Counterexample-driven refinement loop so far (each finding verified
     against code before fixing): (1) ghost row after unobserved
     ExternalDelete -> motivated the completion machine itself;
     (2) `MoveRowRewrite` released the move lease it never required ->
     fixed (ee5adc95): guard requires the held lease on both endpoints
     plus the FS post-move state (rewrite follows the rename); (3)
     `RotateFinish` same lease omission, and `WorkerSkipUnchanged`
     compared only the metadata projection while the code always
     rewrites the identity record on apply (flush_tree.rs; no
     unchanged-skip exists at worker level, hash short-circuit deferred
     per REQ-1) -> fixed (dbd9f0e8): skip guard requires the identity
     record's contentHash (idempotence abstraction: skipping is sound
     only when applying would write nothing new); RotateFinish requires
     the held lease. Class-A/B audit clean beyond these; WorkerSkipStale
     noted as an over-approximate drop (broader snapshot than the code's
     mtime/len) — can only delay convergence, documented, unchanged.
     Probes confirm both falsifiers are disabled at the added conjuncts;
     sim search (10k samples, 30 steps) clean.
3. Scope entry: SCOPE-13 "Nondeterministic scheduling" — any action (FS
   event, API call, fault, pipeline step, Skip) may fire per tick; no
   fairness. API calls are environment events like FS events; the model's
   inter-locking mirrors code reality only (generations, leases, same-txn
   writes). Minimal-interlocking audit rule: each action models exactly the
   atomicity the code provides — no added coordination, and existing locks
   are never stripped to make invariants hold. Lifting: fairness-aware
   scheduling (alongside SCOPE-3).
4. Branch-completeness check in `utils/quint/coverage`: parse the step
   module's `any { ... }` block, compare referenced action names against
   all `action` declarations in the imported modules, fail on any missing
   action. Makes a forgotten branch a gate failure instead of a silently
   narrowed proof. Fixture cases in the checker selftest.
5. Backend bake-off, measured per slice: Apalache (default backend;
   measure the required `--max-steps` — crash-heal chains likely exceed the
   default 10; JRE 17+/21 and version pinning are CI prerequisites) vs
   `quint verify --backend=tlc` (explicit-state; expected to explode on the
   full intake-set universe — one measurement on reduced bounds to confirm).
   Sanity ladder first: `quint typecheck` → step-composed `run` scenarios →
   `quint run --invariant` → only then verify.
   Datapoints so far (on the pre-completion-machine invariant, still
   informative for runtime): Apalache 0.62.4 auto-downloaded by quint into
   `~/.quint/`; needs a JRE (no system java — a portable Temurin 21 JRE
   unpacked to `~/.quint/jre` works: `PATH=~/.quint/jre/bin:$PATH`);
   depth-6 BMC on the 248-branch step ≈ 58 s and it found the expected
   ghost-row counterexample (ExternalDelete with no watcher delivery →
   quiescent but not converged) — exactly the shape that motivated the
   completion machine. TLC not yet measured (no JRE needed; postponed
   until the invariant is final).
6. Run on a nightly lane only; `quint run` stays the merge gate. Claims quote
   the SCOPE register and the step bound.
7. Deferred: eventual-consistency full-model bake-off, inductive invariants,
   liveness/temporal properties.

## Parameterization spike (2026-11-16, branch invest/quint-param)

Verdict: **mechanism B (generated instance copy)**. Quint 0.33.1 has module
instances (`import M(PATHS = ...).* from "..."`) that bind correctly per
instance, but instances are copies, not shared bindings: a state variable
instantiated twice in one module graph becomes two distinct variables
(QNT502 "Variable not set" when another module reads it), and `export` of an
instance re-resolves the prototype, losing the substitution (QNT500).
Qualified access (`import M(...) as C`, `C::x`) works as one coherent
instance but would require rewriting ~2,000 state-reference sites to
`C::name`. Applied to a copy of the real pipeline, the star-import route
fails the canonical-behavior constraint (goal predicate reads a different
instance's variable). Flattener source confirms: imports/exports flatten
before instances. Checker note: step.ts's import regex does not see
instance imports (B needs no checker change).

Reduction surface is wider than the four constants — hardcoded to canonical
bounds: state `init` maps, `initialId`, `rootOf`'s p5 branch,
`dupeIndex`/`thumbPresent` key maps, `nextAssetId' = 6`, `SUBTREE_PAIRS`
literals, and the step enumerations naming p1..p5. A generated instance must
regenerate these, so the recommended sequence is: (1) make the canonical
specs bound-agnostic where cheap (derived init maps, rootOf, SUBTREE_PAIRS),
(2) decide the step-branch form (literal enumeration vs nondet-parameterized
wrappers), (3) generator + staleness gate for the instance.

## A-merged spike, round 2 (2026-11-17, scratch only — repo unchanged)

Tested the merged-one-module design at scale (scripted merge of all six
pipeline files into one 3,005-line module, 182 declarations, 0 collisions;
typed consts `const PATHS: Set[str]` bound via
`import pipeline_merged(PATHS = ...).* from "./merged"`; typecheck passes
with consts unbound).

Results:

- Canonical equivalence: 68/68 runs pass, run-name set identical to the
  modular layout, wall time 314 s vs 317 s, seeded trace state-for-state
  identical, typecheck 15.7 s (vs 19.8 s for the two files). Merging does
  not slow the simulator.
- Reduced instance: only 8 derived-content sites need patching (rootOf,
  SUBTREE_PAIRS, album itemCount, init assetById/assetByPath/dupeIndex/
  metadataPayload/nextAssetId). initialId, thumbPresent/dupeIndex key
  domains and the intake fallback item need no patch at 3-path bounds.
- Run suites are bound-specific in three dimensions: 24/68 runs fail at
  reduced bounds — 8 by out-of-bounds path literals, 15 by per-root
  cardinality (fixed-length drain blocks sized to canonical root
  membership), 1 by root reassignment. This couples the reduction impact
  to run-suite regeneration regardless of layout choice.
- Checker impact: `steps` needs a per-step action-universe notion (the
  merge puts `step` (33 actions) and `stepCompletion` (54) in one file;
  a union rule would weaken the gate — rejected; per-step annotation is
  the fix). `docs` hardcodes two filenames + a run-count line and needs
  entry parameterization. `specs` traceability regenerates mechanically,
  but const declarations escape the declaration regex, so a scope cited
  only by a const would become unreferenced (SCOPE-2 keeps 3 refs — OK
  today).
- Correction: the checker's "1507 branches" figure is an identifier-token
  count, not branch entries. Actual branch entries: 141 (`step`) and 381
  (`stepCompletion`), 170 of which name p4/p5 and are inert at reduced
  bounds (guards disable them).
- Bonus: one merged entry executes 68 runs where today's two entries
  execute 97 (the 33 base runs run twice today).
- Co-importing two instances of the merged module fails hard (QNT101) —
  the round-1 silent cross-instance hazard is tool-enforced away.

Decision: A-merged recommended, sequenced: (1) bound-agnostic derived
content (8 sites, required under either option), (2) merge + thin
entries, (3) `steps` per-step universe annotation, (4) `docs` entry
parameterization, (5) reduced run-suite regeneration only where a
reduced verify target needs it. B in reserve only if multiple bound
variants are needed before (1) lands. Unmeasured: HASHES/SMALL
const-binding generality (their seed maps hardcode 0..2); git-blame
flattening across 3,157 lines is a real review cost.

## Reduction implementation (2026-11-17, branch invest/quint-param)

Steps 1-2 of the A-merged sequence landed:

- 0f8551f0: merge cutover — one 3,174-line `pipeline_merged` module +
  `entry-canonical.qnt`; six modular files removed; `@step-universe`
  annotations (step: 33, stepCompletion: 54, union covers all declared
  actions); `docs` run counts derive from named sources; selftest 25;
  `just quint test` now 68 runs (the 33 base runs ran twice before).
- 90572afc: bound-agnostic derived content — init maps/dupeIndex/
  metadataPayload fold over PATHS via the existing initial* defs,
  nextAssetId = PATHS.size()+1, itemCount = PATHS.size(), rootOf via a
  single instance-data val SECOND_ROOT_PATHS, sidecar draws
  SMALL.exclude(Set(0)); SUBTREE_PAIRS isolated as instance data;
  initialId left explicit (future const). Equivalence verified by REPL
  against canonical maps; 68 runs green.
- 01925365: consts (PATHS/HASHES/SMALL/SECOND_ROOT_PATHS/SUBTREE_PAIRS) +
  entry-reduced-paths (PATHS only) + entry-reduced (full: HASHES=SMALL=
  0.to(1)); thumbPresent seed was a hardcoded straggler, now
  HASHES.mapBy(h => h == 0). Generality: no other stragglers; purity
  clean. Reduced run counts: 44/24 pass/fail IDENTICAL under both
  reductions — no run depends on hash/snapshot value 2 beyond the path
  failures (8 path literals, 15 per-root cardinality, 1 root
  reassignment). Reduced entries are verify vehicles, not regression
  vehicles.

## TLC round: totality gap + state-space reality (2026-11-17)

- Bake-off ladder at reduced bounds: TLC d12 failed in 5 s at 630k
  states/min with an out-of-domain application; Apalache d8 was killed at
  38 min still at depth 3 — the bounds reduction did not move Apalache's
  per-state cost (dominated by the 1,648-branch transition relation, not
  the bound universe). Ladder short-circuited.
- Backend-semantics asymmetry (recorded): TLC does NOT short-circuit `or`
  around function application and ERRORS on out-of-domain application;
  Apalache's SMT encoding proceeds with an unconstrained value. TLC is
  the stricter oracle; canonical bounds masked the gap.
- Refinement round 4 (391eee9c): the gap was the dir-move resolvability
  foldl (`not(contains) or contains(get)` evaluated eagerly) in
  ExternalRenameDir/ApiAssignDirMove; fixed with a lazy conditional.
  Sibling audit fixed four more unguarded reads (dupeIndex/thumbPresent/
  assetById keys in WorkerApplySuccess, WorkerSkipUnchanged,
  WorkerRemoveApply, ApiRemove). 68 runs green; all gates green.
- Post-fix TLC: no model error; 19M distinct states at depth 5, 65M at
  depth 6 (5.2 min, then quint's internal timeout). The reduced state
  space explodes with depth (nondet content draws multiply it) —
  exhaustive deep verification is infeasible for BOTH backends at any
  practical bound configuration. The principled escape is an inductive
  invariant (TypeOK-style strengthening); the shallow-depth TLC lane is
  the practical nightly claim.

## Spike: prefix-partition verification (2026-11-17, next)

Direction (user-confirmed): shallow-depth exhaustive checking is vacuous —
free events + drain + the scan cycle need ~10-20 steps, so depth-5/6 BMC
never reaches a checked state. Instead: partition the free-phase event
prefix (phase 0), bake each partition element into a per-prefix init
(`initCompletion.then(e1)...then(BeginDrain)`), and verify the suffix
(drain + mandatory scan cycle) exhaustively per prefix with TLC — unbounded
depth over everything that happens after the events. The phase machine
already supports this: phase-0 branches are `phase == 0`-guarded, so an
init assigning `phase = 1` (or ending in BeginDrain) IS the suffix-only
machine; no new step action is needed.

Claim shape: "for every prefix in the partition Pi, every completion
converges" — stronger than depth-bounded BMC (vacuous here) and MBT
(sampled prefix, no exhaustive suffix). Pi is machine-generated and
auditable; a violation names its exact event prefix.

Partition policy (user-selected): all singletons (every free-phase branch
once), pairwise event-CLASS combinations (systematic over ~28 classes with
representative argument instances, not all ~250^2), and hand-listed
adversarial triples from the events-catalog fault interactions. No seeded
random batches.

Next steps:

1. Harness: prefix generator (from the action inventory the steps checker
   already computes) emitting per-prefix init actions into a module chained
   on the reduced entry; runner looping `quint verify --backend=tlc` per
   prefix with outcome/runtime/states aggregation; harness committed as
   proof-lane infrastructure (file args, no hardcoded paths, house TS/mjs
   patterns).
2. Measurement subset (~90 prefixes): ~30 singletons (one per class),
   ~50 pairwise class pairs, ~8 adversarial triples. TLC first; then
   Apalache on a smaller subset for the symbolic column.
3. Verdict: per-backend feasibility and runtime distribution; violations
   become refinement-round findings; SCOPE-15 (prefix partition policy)
   lands when the lane becomes a real target.
4. Lane home: nightly/manual-dispatch CI job; free-tier 6 h/job is ample
   at measured runtimes (fits devops-cleanup's CI-lane item).

## Prefix-partition spike results (2026-11-17, commit 10b99b03)

Harness landed: `coverage/bin/prefixes.ts gen|run` (gen emits per-prefix
init actions ending in BeginDrain from the phase-0 branch inventory, class
grouping with representative instances, adversarial triples from a
hand-listed file; run loops quint verify per prefix with
outcome/runtime aggregation). Selftest 26; all gates green; model
untouched.

- **TLC: blocked for this pattern, hard.** TLC rejects action composition
  in init outright ("The current version of TLC does not support action
  composition"), and QNT409 blocks helper actions shared between multiple
  init actions in one module even when inlined under fresh names. No
  measurement obtainable; a model-level per-prefix refactor cannot remove
  the composition itself. TLC is not the backend for prefix-partition
  verification.
- **RETRACTED (2026-11-18): "12/12 prefixes converge" was a measurement
  artifact.** quint exits 0 with no verdict when Apalache's preprocessing
  fails; the old exit-status classifier read that as ok. With output-based
  classification (ab198754), all measured prefixes are errors: Apalache
  0.62.4 rejects action composition (`\cdot`) in the init predicate —
  confirmed with a plain-TLA+ control independent of Quint — so every
  composed-prefix init fails in PreprocessingPass before any exploration.
  No convergence claims for the model exist yet. Lesson recorded: classify
  from output markers, never exit status alone.

Feasibility: full partition policy ≈ 630+ prefixes x ~206 s ≈ 35 h
single-job — exceeds the 6 h CI budget as-invoked. The lever is Apalache
server-mode amortization (one long-lived server, per ADR008
--server-endpoint): startup is ~all of the 206 s, so amortized per-prefix
cost is plausibly seconds-to-tens-of-seconds; measure before sizing the
lane.

First genuine convergence claims for the model: the completion machine
converges under the adversarial event set at reduced bounds (scoped:
subset partition, reduced instance, bounded invariant).

Next: (1) measure server-mode amortized per-prefix cost; (2) size the
full-policy lane against the CI budget (shard by partition class if
needed); (3) land SCOPE-15 + the nightly/manual-dispatch lane
(devops-cleanup CI item); (4) violations from the full run become
refinement rounds.

## Class-level spike round 2 (2026-11-18, commit ab198754)

- Apalache 0.62.4 does not support `\cdot` in the init predicate (plain
  TLA+ control fails identically) — every composed prefix init is
  rejected before exploration. Also surfaced: `nondet` requires oneOf
  (QNT204), `.then` requires identical effect sets (QNT000 — the Stay
  full-assignment shape is mandatory), an init denoting no state surfaces
  as a step-0 deadlock with an empty state (the vacuous detector keys on
  that), and `quint compile` cannot inspect the encoding (QNT409).
- Server-mode amortization is ~0: the 205 s per prefix is Snowcat
  typechecking (~190 s of it) rerun per request; JVM start is 3-5 s.
  Requests serialize on the server. Server mode is not the lever.
- Cost breakdown: SanyParser 2.7 s, Snowcat 190 s, passes 2-9 ≈ 2 s,
  client-side flatten/IR ≈ 25 s. A 3-init module trims Snowcat only to
  156 s — intrinsic to the merged model size.
- Class-level partition generated: 106 elements (50 classes, 35 pairs,
  14 same-class pairs, 6 adversarial) — measured subset all `error`
  (7/7) for the composition reason above; vacuous/violation counts
  unobtainable while composition fails (vacuous detection is
  selftest-covered only).
- CI sizing: ~205 s/prefix ⇒ 430-630 elements ≈ 24.5-36 h serial; a 6 h
  job needs ≥5-8 shards. Runtime levers: sharding, and a run-stripped
  verify module (runs are a large share of the 3,174 lines Snowcat
  typechecks per request).

Encoding options for the next decision (composition is unusable in
init): (i) single-event class-level prefixes via pre-state pinning — emit
`initPk = all { <unprimed equalities to the deterministic initial state>,
nondet args, <Event>, <phase bookkeeping> }`; mechanical, sound, no
composition needed, covers every event class alone; (ii) multi-event
prefixes require substituting event 1's post-expressions into event 2's
reads — engineering-heavy, expression blowup beyond short chains;
(iii) abandon prefix decomposition and pursue the inductive-invariant
route for the deep claim.

## Pinned pre-state round (2026-11-18, branch invest/quint-param)

Encoding option (i) landed as `prefixes.ts gen --pin-prestate` (21417c1f),
but not in the form first written down. Measured facts:

- **Pinning by unprimed equalities does not encode.** Apalache's init
  predicate is single-state: `v == <init expr>` is itself an assignment of
  `v`, so it collides with the event's `v' = <expr>`:
  `Assignment error: <[UNKNOWN]>: Manual assignment is spurious,
real_pin::pipeline_merged::files is already assigned!`
  (TransitionFinderPass, 196 s in). A control init containing only the event
  fails earlier with `x is used before it is assigned` — there is no
  pre-state in an init predicate at all, so no guard/membership variant of
  the equality form can work.
- **Substitution is the equivalent form that survives.** The generator reads
  the model's deterministic `init` assignments, emits them as `INIT_<var>`
  values (derived projections such as `rows` get their own pinned copy) and
  inlines each event class with every state read replaced. Hand-validated
  first on the reduced instance: deterministic `ExternalDelete("p1")` reached
  exploration (no preprocessing error), and the nondet form
  (`nondet p = PATHS.oneOf()`) was found violating a probe invariant at state
  0 with `files.p2.present = false` — i.e. Apalache does explore the multiple
  initial states of one element.
- **TLC accepts it too.** The earlier "TLC does not support action
  composition" blocker is specific to `.then` in init; the pinned init has no
  composition. TLC additionally **ignores `--max-steps`** (verified on an
  unbounded counter: it explored 42M states until the disk quota), so a TLC
  verdict on a pinned element is an exhaustive claim over the finite state
  graph, not a depth-bounded one.
- **Costs at the reduced instance (3 paths, 2 hashes, 2 roots).** Snowcat is
  ~195-207 s per verifier request. Stripping the 68 `run` declarations from
  the model (798 of 3181 lines) does not reduce it (218 s vs 221 s wall to
  first state) — the run-stripping lever does not exist. Apalache exploration
  adds ~90 s (depth 1), ~110 s (depth 2), ~610 s (depth 3), so depth 60 is
  out of reach (~14 h per element projection). TLC pays ~35-50 s per
  invocation for the Quint→TLA compilation and then explores.
- **Full single-event sweep** (51 elements, TLC, 120 s budget per element,
  3 shards with distinct `--server-endpoint` ports because the TLC path
  compiles through an Apalache server): 24 vacuous, 26 timeout, 1 violation,
  0 ok; 74 min serial. Vacuous mean 51.5 s, timeout 120.1 s. The vacuous set
  is exactly the classes whose guards need non-initial state (intake-, lease-,
  sweep-batch-preconditioned worker/move/rotate/sweep branches): a
  single-event prefix cannot reach them.
- **Finding (new refinement round, model-level, reproduced without the pinned
  encoding):** `SweepEnumerate` _replaces_ the global `sweepBatch`, so the
  second root's enumeration discards the first root's queued stale removals.
  Deterministic reproduction on the canonical module:
  `ExternalDelete("p2")` → `BeginDrain` → `BeginScan` → `CompleteScan("r1")`
  → `CompleteScan("r2")` → `SweepEnumerate("r1")` (batch `{p2}`) →
  `SweepEnumerate("r2")` (batch empty) → drain intake → `Quiescent` holds at
  phase 2 with both roots scanned while `files.p2` is absent and `p2` still
  has a row (`¬Converged`). Code check: `sweep_stale_asset_paths` submits one
  `WriteOp::SweepRemoveBatch` per root/chunk to the writer queue, so the
  submissions accumulate; the model's single replaced batch is the gap.
  Candidate fix: `sweepBatch' = sweepBatch.union(...)`.
- **Two-event prefixes (option ii): encoding is feasible, measurement is
  not.** Hand-encoded pairs (`ExternalDelete("p1")` →
  `SubmitWatcherRemove(p)`, `ExternalModify("p1")` → `ExternalModify(p)`)
  typecheck and their post-state assertions pass. The mechanical rule is a
  substitution map `state var → its current post expression`, applied
  iteratively: each event contributes its guards under the current map and
  then rewrites the map from its assignment conjuncts. Expressions stay
  readable at length 2 (`INIT_files.set("p1", {...}).get(p0)`, chained
  `.set`), growth is per-read text, not nesting depth. It buys nothing while
  no non-vacuous single-event element completes a verification run: pairs
  inherit the same per-element cost with a larger state space.

- **No element exhausts at these bounds.** TLC on the empty-prefix element
  ran 40 min without a verdict: 29.5M distinct states at depth 18, 16.3M
  still queued and growing, 9.1 GB of fingerprints. The cost is the shared
  phase-1/2 suffix machine (crashes, rescans and sweeps loop), not the
  prefix: every satisfiable element explores essentially the same graph.
- **One disjunctive init is the cheaper shape.** An init
  `any { initP0, ..., initP50 }` typechecks and TLC computes 126 distinct
  initial states from it (27 satisfiable elements; nondet arguments expand
  them), so a single run carries the whole single-event partition. That run
  found the same sweep-batch violation in 203 s (156 s of it exploration,
  depth 8) where the 51-element sweep needed 74 min to reach the same
  conclusion. From the same initial-state set, a completed exhaustive run
  would be one claim instead of 51.

Verdict: (i) is the right encoding and is now machinery; the missing piece is
verification cost, not the partition. (ii) is implementable but not the lever.
The lever is a cheaper target — an inductive invariant (option iii), or a
materially smaller bound configuration — not more prefix shapes.

## Fundamental insights and direction change (2026-11-18)

Stepping back from measurement levers (shards, bounds, encodings), three
structural levers were under-used:

1. **Fault-free drain is lossless.** Phase 1's action set is a subset of
   phase 0's, and files are frozen in phases 1-2 (no environment). Every
   crash-mid-drain state is therefore free-phase-reachable (partial worker
   application then crash happens in the free phase too). Removing
   ProcessCrash/LoseThumbs/DbLoss from phase 1 loses no coverage: crash
   resilience migrates to the prefix partition, where "crash" is already a
   first-class partition element (fault insertion at the boundary, not
   inside the recovery fixpoint). This removes the crash/rescan churn
   loops that drove the suffix state explosion (29.5M states at depth 18
   for the empty prefix).
2. **Rounds 1-5 are CEGAR by hand — bank them.** Each counterexample fix
   is literally an inductive-invariant conjunct: record resolvability,
   lease-held-before-release, identity-fresh-before-skip, eager-evaluation
   totality, sweep-batch accumulation. The inductive route's real cost is
   collecting and formalizing these conjuncts (`--inductive-invariant`),
   not starting over with TypeOK strengthening.
3. **Assume-guarantee decomposition.** The sweep-batch bug was a local
   3-stage-protocol bug; a standalone sweep-protocol model (2 roots,
   enumerate/recheck/apply) checks exhaustively in seconds. Local models
   for intake, sweep, move-lease give cheap exhaustive checks feeding the
   global claim.

Secondary: partial data independence (mtime is compared wholesale and can
fold into a snapshot version; hash keys dupeIndex, len is summed into
album item_size, sidecar is REQ-1 identity — those keep their roles);
TLC symmetry reduction (paths within a root); Apalache invariant
splitting.

### New direction

1. Model fixes first: (a) per-root sweep-batch accumulation (round 5;
   SCOPE-8/9/11 revised), (b) fault-free drain phase (SCOPE-13/phase
   machine revised; crash resilience reframed to partition level).
2. Make the disjunctive init a first-class generator output (the
   126-initial-state single invocation found the sweep finding in 203 s
   vs 74 min per-element).
3. Re-measure the single-event partition on the reduced instance:
   TLC for exhaustive verdicts (may still be heavy), Apalache bounded
   depth for termination; the green-cost datapoint decides lane
   viability.
4. SCOPE-15 + lane registration after the first green; nightly/manual
   dispatch per devops-cleanup.
5. Inductive-invariant skeleton: record the conjunct list (from rounds
   1-5) in the plan; pursue only if greens stay unaffordable.
6. Cleanup: the composed-init gen modes (instance/class-level without
   pinning) are unencodable on both backends — superseded by
   --pin-prestate; strip or clearly mark dead.

## Step/action division of labor (review anchor)

- `step`: which action may fire per tick — uniform choice over enabled
  branches; no phase sequencing, no ordering constraints (they would shrink
  the reachable set and overstate the proof).
- Action guards: whether an action fires (lease held, file present, intake
  non-empty). All mutex/ordering semantics live here already.
- Action assignments: what fires atomically (same-txn move writes,
  generation bumps).
- A missing lock cannot be patched at step level; an action that should hold
  a mutex but does not is a finding, not a modeling gap.

## Decisions

- 2026-11-15: scope set after reading quint.sh checking-properties /
  model-checkers docs and measuring tool state (quint 0.33.1 ships
  `--backend=tlc`; Apalache is actively maintained under apalache-mc, not
  deprecated). Brief: `docs/briefs/quint-modeling.md`.
- 2026-11-15: spike work happens in the `invest/quint-mbt` worktree;
  mainline merge only after the backend data is in.
- 2026-11-15: API calls are environment actors (user-confirmed); the
  system under proof is the pipeline. Coupling only via the code's real
  mechanisms (generations, leases, same-txn writes).
- 2026-11-15: mutexes/ordering live in action guards, not in `step`;
  `step` adds no ordering. Branch completeness is machine-checked
  (coverage tool), not trusted by hand.
- 2026-11-15: `Settled` needs the completed-scan premise (G1); lean
  option (a) additive scan watermark, decided after the spike shows the
  plain quiescence-invariant violation shapes.
- 2026-11-16: step module landed (63e843ea, 3cfba474, 9135e7ca). Totality
  guards accepted as two classes: projection totalization (torn states must
  be representable) and resolvability/liveness preconditions mirroring code
  re-checks (REQ-7, SCOPE-11/12). `WorkerApplySuccess`/`UploadIndexed` use
  resolvability-conditioned reads instead of disabling guards, mirroring
  flush_tree.rs `existing_id`/`old_record = None` re-use-or-heal — a
  disabling guard would deadlock the dangling-path-key + present-file shape
  (sweeps only enumerate absent files). Album aggregate row id 0 confirmed.
- 2026-11-16: open fidelity question flagged (not changed): on torn
  re-index, the model's album itemCount delta keys off
  `assetByPath.contains(p)` while flush_tables decides add-vs-update from
  `old_album_id` (None → re-add member, +1). No assertion covers aggregates
  on torn states.
- 2026-11-16: phased completion machine landed in `asset-aspects-step.qnt`
  (replaces `stepAspects`/`Settled`): `phase` 0 free → 1 draining (crashes,
  untracked scans) → 2 scanning (one tracked `CompleteScan` per root), with
  `CompletionInvariant` claimed at phase-2 quiescence. Scan triggers mix with
  the drain and crashes by design; only the final tracked cycle is assumed
  clean. The steps checker exempts `init<Name>` entry actions.
