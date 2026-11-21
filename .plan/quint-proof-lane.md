---
status: open
type: feature
priority: medium
area: testing
---

## Summary

Proof exploration on the quint-model-linkage model layer (branch
`invest/quint-param`, forked from the landed mainline work). Outcome after
five days of staged exploration: **no machine-checked convergence verdict
was obtained**, and the record below shows why, with measurements at every
step. The exploration produced durable value regardless:

- **One real model bug found, fixed, regression-pinned** (round 5:
  `SweepEnumerate` replaced the global sweep batch, discarding the other
  root's queued stale removals — two-root sweep interleaving did not
  converge; reproducible in a plain `quint run`, fixed TDD with a
  mutation-checked test).
- **A faithful, mutation-checked fault model** (crash-free request phase
  justified by re-derivability; budgeted drain crashes with
  reboot-to-request semantics; uninterrupted heal cycle).
- **A 70-run regression suite** on the merged model, plus the merged
  single-module model with bound-agnostic constants and a reduced
  verification instance.
- **An empirical map of where the verification complexity lives**: the
  drain+heal interleaving product, not the event space — established by a
  sequence of falsified hypotheses, each with a measurement.

Three follow-on options remain, priced in "Follow-on options" below. The
exploration phase is concluded; this file is the record and the decision
basis for what (if anything) lands.

## Initial plan (2026-11-15, as approved)

Starting point: the model layer on quint-model-linkage — JSON catalogs
(goals/assumptions/scope/requirements/events), generated docs, 25
test-linked properties, `quint run` at 10k samples per scenario. Sampled
execution gives confidence, not verification. The approved plan:

- Extend the `step` action to cover all 16 asset-aspects actions plus a
  `Skip` stutter; re-express run assertions as a settled-state invariant;
  add a branch-completeness gate to the coverage checkers.
- Run `quint verify` (model checking) on the asset-aspects slice first;
  backend chosen by measurement (quint 0.33.1 ships Apalache default and
  `--backend=tlc`). Safety-only: settled-state invariants, no
  liveness/fairness. Bounded model checking as the intended stop.
- Nightly lane, never a merge gate; claims quote the SCOPE register.

Validation framing (user-confirmed): the environment is the set of
external actors — filesystem events, API calls, faults — firing at any
time; the system under proof is the pipeline machinery. Goal: eventual
consistency in the as-if-serial sense — once the pipeline drains, DB and
derived state agree with the FS record (ASS-3/G5), modifications applied
in generation order, none lost.

Starting-point gaps that shaped the work: the aspects actions were in no
step; no stutter branch; no settled-state invariant form; no
branch-completeness enforcement; nothing wired to `quint verify`.

## Exploration record

Ordered as actually explored. Each entry: what was done, the problem
encountered, the measurement that settled it.

### 1. Completion machine (landed; prerequisite for everything else)

Built the phased state machine in a step module over the model:
phase 0 free (any action) → 1 draining → 2 tracked heal cycle
(`CompleteScan` once per root), with `CompletionInvariant` claimed at
phase-2 quiescence: `(phase = 2 ∧ every root scanned ∧ Quiescent) ⇒
(Converged ∧ AspectsAligned)`. Design refined twice with the user: scan
triggers mix with events and crashes (only the final cycle is assumed
uninterrupted); the crash-resilience and multi-part-transaction
discussion settled the fault semantics (see exploration 5).

Problems found and fixed along the way (each a TDD round with a
reproducing run and a code anchor):

1. Ghost row after an unobserved external delete — falsified the first
   plain quiescence invariant; motivated the phase machine itself.
2. `MoveRowRewrite` released the move lease it never required — guard now
   requires the held lease on both endpoints plus the FS post-move state.
3. `RotateFinish` same lease omission; `WorkerSkipUnchanged` compared
   only the metadata projection while the code always rewrites the
   identity record on apply (`flush_tree.rs`; no unchanged-skip exists at
   worker level, hash short-circuit deferred per REQ-1) — skip guard now
   requires identity match.
4. Eager out-of-domain evaluation in the dir-move resolvability `foldl`
   (`not(contains) or contains(get)` evaluated against an empty table) —
   found by TLC at reduced bounds; fixed with a lazy conditional; sibling
   audit fixed four more unguarded reads.

### 2. Backend reality (measured)

- Apalache (0.62.4, symbolic, bounded `--max-steps`): per-depth
  exploration cost on a single element 90/107/610 s at depths 1/2/3;
  Snowcat typechecking ~195–207 s per verifier request regardless of
  model content (run-stripping 798 of 3,181 lines: 218 s vs 221 s — the
  run-stripping lever does not exist). Bounded runs terminate; deep runs
  do not fit any budget.
- TLC (explicit-state): **ignores `--max-steps`** (verified on an
  unbounded counter toy: 42M states until disk quota) — a TLC verdict is
  an exhaustive claim over the finite state graph, or non-termination.
  Throughput ~0.6–6M distinct states/min depending on layer.
- Measurement-integrity lesson: quint exits 0 **without printing a
  verdict** when Apalache preprocessing fails; an exit-status classifier
  read that as "12/12 prefixes converge". Retracted; classifier now
  output-based (a committed selftest pins it).

### 3. Input-shaping attempts (all measured; none sufficient)

- **Bounds reduction** (merged single-module model — see "What landed";
  reduced instance 3 paths / 2 hashes / 2 roots via typed consts): only
  8 derived-content sites needed patching once init/root content was
  derived from the bound constants. Run suites are bound-specific in
  three dimensions (24/68 runs fail at reduced bounds: 8 out-of-bounds
  path literals, 15 per-root drain-block cardinality, 1 root
  reassignment) — reduced entries are verify vehicles, not regression
  vehicles.
- **Crash budget K=1** (drain crashes bounded, reboot returns to the
  request phase): removed crash/rescan churn loops. K=0 diagnostic
  (mutating all 9 budget guards, regenerating, TLC 30 min): still 16.2M
  distinct states at depth 14 — the churn loops were a contributor, not
  the driver. Request-phase budget (R) proposed for the reboot event
  fan-out; **demoted before implementation** — see exploration 4's
  empty-prefix probe.
- **Data independence** (collapse `(hash, mtime)` into one version
  token; `len` stays numeric for album `item_size` sums, `sidecar` stays
  for REQ-1 identity): proposed, not implemented — overtaken by
  exploration 4's finding that the explosion is not in the event
  content.

### 4. Prefix-partition verification (the main design; machinery landed, verdict unaffordable)

Design (user-selected policy): partition the request-phase event space —
all singletons, pairwise event-class combinations, hand-listed
adversarial triples — bake each element into a per-element init, verify
the drain+heal suffix exhaustively per element. Avoids the vacuity of
shallow-depth BMC (free events + drain + scan cycle need ~10–20 steps).

- **Composed-prefix encoding is unencodable**: Apalache 0.62.4 rejects
  action composition (`\cdot`) in the init predicate — confirmed with a
  plain-TLA+ two-variable control independent of Quint. TLC rejects it
  outright ("does not support action composition") and QNT409 blocks
  shared helpers. No generator workaround exists; the limitation is in
  the checkers, not the model.
- **Pinned substitution encoding works** (the equivalent form): the
  generator materializes the deterministic init into `INIT_<var>`
  values and inlines each event class with every state read substituted.
  Hand-validated; Apalache explores the multiple initial states of a
  nondet element; TLC accepts it and ignores `--max-steps` (verdicts are
  exhaustive or non-terminating).
- **Single-element sweep** (51 elements, TLC, 120 s budget, 3 shards,
  74 min serial): 24 vacuous (guards needing non-initial state —
  intake/lease/sweep-batch preconditioned branches), 26 timeout, 1
  violation, 0 ok.
- **The violation is the round-5 model bug**: `SweepEnumerate` replaced
  the global sweep batch. Reproduced canonically (plain `quint run`,
  no special encoding): delete p2 → both roots scanned → `SweepEnumerate`
  r1 (batch `{p2}`) → r2 (batch emptied) → drain → quiescent, file absent,
  row present, `¬Converged`. Code check: `album_index.rs` submits one
  `WriteOp::SweepRemoveBatch` per root/chunk and submissions accumulate;
  the model's single replaced batch was the gap. Fixed TDD (see
  exploration 5's summary row above and "What landed").
- **Disjunctive init**: one action `any { initP0..initP46 }` covering the
  whole single-event partition — 126 distinct initial states; found the
  same violation in 203 s (156 s exploration, depth 8) vs 74 min for the
  per-element sweep. This is the right measurement shape.
- **The empty-prefix probe (the decisive falsification)**: TLC on the
  empty prefix alone (one deterministic initial state, no event, no
  crash, no content draws) at reduced bounds: 41M distinct states by
  depth 7 (234k at depths ≤6, then 6M→45M per depth), still growing at
  ~5M/min at the 10-min cap. A follow-up K=0 full-partition run: 16.2M
  distinct at depth 14 in 30 min. **The explosion is the drain+heal
  interleaving product itself** (scan-item apply orders × sweep-stage
  interleavings × argument-enumerated branches), independent of events,
  crashes, content, and partitions. This falsified the reboot-fan-out
  attribution and demoted the request budget, the data-independence
  collapse, and further partition shaping in one measurement.

### 5. Inductive-invariant route (spiked; blocked by model representation)

The structural alternative: no state enumeration at all — check
`init ⇒ I`, `I ∧ step ⇒ I'`, `I ⇒ goal` symbolically (Apalache
`--inductive-invariant`, 2–3 SMT passes). Cost is proportional to the
transition relation (~372 transitions), not the state count; nondet case
splits are handled natively. The five CEGAR rounds above already
discovered most of `I`'s conjuncts (resolvability, lease-held,
identity-freshness, eager-evaluation totality, batch accumulation), and
the re-derivability argument becomes a conjunct that removes the fault
budget entirely.

Spike result (rung 1 of the ladder — TypeOK alone): the **invocation
form works** (base case PASS: 42 verification conditions, ~190 s;
positive control on the published mutex example: green, 9 s), but **the
model's state representation is not encodable** for Apalache's inductive
checking. The invariant must domain-assign every state variable at first
occurrence, and Apalache accepts maps only as **total maps over a
constant finite key set with a finite value set** (`setOfMaps`). Measured
rejections: infinite value sets, infinite key sets, partial maps. 8 of 16
state variables have no domain expression — the six sparse tables
(`assetByPath` 252, `metadataPayload` 147, `assetById` 123, `dupeIndex`
60, `sweepBatch` 52, `thumbPresent` 44 action-body mentions) and two
int-valued total maps (`files`, `generations`). Rungs 2–4 (goal
inductiveness, banked conjuncts, debt sketch) were not measurable behind
this gap. `generations` is the creative item: it is both a counter and an
ordering (`it.generation < generations.get(p)`), so a finite domain needs
an inductive no-spread argument or an explicit token relation replacing
the order. Estimated reshape cost: 2–3 weeks (representation + counter
design, ~680 action-body mentions, 70 runs to revalidate) plus 1–2 weeks
of invariant iterations.

### 6. Assume–guarantee local models (not tried; priced)

Verify the sweep chain, crash recovery, and intake protocol as small
standalone models — each exhaustively checkable in seconds — with the
global claim resting on local proofs plus a shallow global check. The
round-5 bug was a local sweep-protocol bug; a local sweep model would
have caught it before the global model existed. Estimated: days.

## Measurement summary

| Configuration                                                     | Method                             | Result                                                                                                                        |
| ----------------------------------------------------------------- | ---------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Full machine, reduced bounds, disjunctive init (47 elements), K=1 | TLC, exhaustive, 60 min cap        | 273M distinct states at depth 6, ~4M/min growth, 44 GB pool; no violation; non-terminating                                    |
| Same                                                              | Apalache, bounded `--max-steps 60` | non-terminating at 50 min, before depth 3                                                                                     |
| Empty prefix alone, K=0 (monotone machine)                        | TLC, exhaustive, 10 min cap        | 41M distinct states by depth 7 from one deterministic initial state                                                           |
| Full partition, K=0                                               | TLC, 30 min cap                    | 16.2M distinct at depth 14; non-terminating                                                                                   |
| Single-element Apalache exploration                               | bounded, depths 1/2/3              | 90/107/610 s per depth                                                                                                        |
| 51-element single-event sweep                                     | TLC, 120 s/element, 3 shards       | 74 min: 24 vacuous, 26 timeout, 1 violation (= round-5 bug), 0 ok                                                             |
| Disjunctive init (126 initial states)                             | TLC, single invocation             | same violation in 203 s                                                                                                       |
| Apalache inductive check, TypeOK base case                        | 3 SMT passes                       | PASS (42 VCs, ~190 s); step phase blocked by map-domain encoding                                                              |
| Snowcat typechecking                                              | per verifier request               | ~195–207 s; run-stripping does not reduce it                                                                                  |
| Empty-prefix drain probe, faults=1, uncapped (2026-11-20)         | TLC, 600 s cap, reduced bounds     | 4,316,486 distinct, 2,314,698 queued, depth 20 climbing; no verdict                                                           |
| Same, `genSum < 8` on phase-1 ScanRoot r1/r2                      | TLC, exhaustive, reduced bounds    | 16,670 distinct, queue 0, complete depth 29, 36 s; `[ok] No violation found`                                                  |
| Full reduced control with R=8, G=12, freshness, id quotient       | TLC, 300 s cap                     | 12,048,992 distinct, 11,710,729 queued, depth 6; timeout, no verdict                                                          |
| Same, one freshness conjunct removed                              | `quint test` 70 runs + TLC 300 s   | suite 70/70; TLC 12,965,361 distinct (+8%), depth 7; timeout, no verdict                                                      |
| Same, one phase-0 `genSum < 12` conjunct removed                  | TLC, 300 s cap                     | 11,122,895 distinct (−8%), depth 6; timeout, no verdict. Growth expectation did not hold                                      |
| R=0 mutation, canonical suite                                     | `quint test` 70 runs               | 70/70; no completion run failed. Rescan budget is not exercised by deterministic runs                                         |
| T1 attempt, kitted model (2026-11-20)                             | `prefixes.ts gen`                  | blocked: generator expects one non-phase conjunct; SCOPE-14/15 guards add one or two. No verifier run; harness fix authorized |
| Generator fix `9ae4c749`                                          | `prefixes.ts gen`                  | 46 classes / 47 elements on the kitted pipeline; lint/selftest/check/70-run/pre-commit gates green                            |
| T1 disjunctive init, kitted model                                 | TLC, single `initAllPinned`, 2 h   | timeout: 1.47B generated / 402.6M distinct / 392.7M queued; no marker; 74 GB scratch tripped disk guard                       |
| T2 per-class fallback, kitted model                               | `prefixes.ts run`, 600 s/class     | 47/47 QNT409 compile errors (`initP<k>` both init and step via `initAllPinned`); 0 model verdicts                             |

## What landed on `invest/quint-param`

All model fixes are bug fixes against quint-model-linkage semantics and
are mainline-worthy regardless of the proof decision:

- Merged single-module model (`pipeline_merged`, ~3,174 lines, 182
  declarations) with bound-agnostic derived content and typed bound
  consts; `entry-canonical` / `entry-reduced-paths` / `entry-reduced`
  entries. Run suite: 70 runs (the modular layout executed 97, running
  the 33 base runs twice).
- Model fidelity fixes, rounds 1–5 (TDD with mutation checks): totality
  and resolvability guards; move/rotate lease-held preconditions;
  skip-unchanged identity freshness; eager-evaluation fix;
  **sweep-batch accumulation** (the round-5 bug).
- Fault model: crash-free request phase (re-derivability), budgeted
  drain crashes with reboot-to-request semantics, uninterrupted heal
  cycle; `drainCrashRecoveryTest` + retargeted
  `completionCrashScanHealsTest`; SCOPE-8/9/11/13 revised.
- Checker gates: per-step `@step-universe` annotations (branch
  completeness made per-step), derived docs run counts, output-based
  verifier outcome classification, pinned-prefix generator
  (`prefixes.ts gen`, disjunctive `initAllPinned`), runner hardening
  (process-group kills, server-endpoint pass-through).
- SCOPE-13 (nondeterministic scheduling) and the phase-machine design
  recorded in the scope register.

## What was not obtained

- No machine-checked convergence verdict, at any bound configuration,
  for any subset of the model. The measured reason: the drain+heal
  suffix interleaving product explodes independently of the inputs
  (exploration 4), and the inductive route is blocked by the model's
  sparse-table representation (exploration 5).
- The inductive-strength gap (how far the banked conjunct skeleton is
  from a full invariant) is unmeasured — it sits behind the
  representation redesign.

## Open item carried from the exploration

Unresolved fidelity question (flagged 2026-11-16, not changed): on torn
re-index, the model's album `itemCount` delta keys off
`assetByPath.contains(p)` while `flush_tables` decides add-vs-update
from `old_album_id` (`None` → re-add member, +1). No assertion covers
album aggregates on torn states.

## Follow-on options (priced)

- **(A) Representation redesign → inductive proof.** Admissible tables
  (total maps with sentinels or `Set[Record]`), bounded universes,
  generation-order redesign; ~680 action-body mentions + 70 runs to
  revalidate; then 1–2 weeks of invariant iteration on the banked
  conjunct skeleton. 3–5 weeks total. Delivers the unbounded convergence
  claim (no budgets, no partition register; the proof covers canonical
  bounds). The reduced instance stays useful for testing only.
- **(B) Assume–guarantee local models.** Days. Exhaustive local claims
  for the sweep chain, crash recovery, intake protocol; global claim
  rests on local proofs + a shallow global check. The cheapest path to
  real verified statements about the bug-prone protocols.
- **(C) Close the lane.** Keep the model fixes, the 70-run suite, and
  the harness as regression assets; record this file as the finding.
  The 2026-11-15 plan's verification goal is not met; the honest claim
  remains "sampled execution + 70 scenario runs + five TDD'd model
  fixes".

## Step/action division of labor (design anchor)

- `step`: which action may fire per tick — uniform choice over enabled
  branches; no ordering constraints in the step (they would shrink the
  reachable set and overstate the proof).
- Action guards: whether an action fires (lease held, file present,
  intake non-empty). All mutex/ordering semantics live here.
- A missing lock cannot be patched at step level; an action that should
  hold a mutex but does not is a finding, not a modeling gap.

## Method findings (2026-11-20)

The bounded lane was closed on a method mismatch, not a budget shortfall:

- `CompletionInvariant` (`utils/quint/pipeline/merged.qnt:2696-2712`) is a
  quiescence-guarded **safety implication**. It does not assert that the
  antecedent is reached; the spec has no `WF`/`SF`, so infinite stuttering is
  permitted (`merged.qnt:272-276`). A terminating TLC pass proves “no bad
  quiescent state in the checked graph,” not convergence.
- A non-terminating exhaustive run proves nothing. T1 reached 1.47B generated /
  402.6M distinct / 392.7M queued in 2 h, with the frontier 97.5% undrained and
  growing at ~3.35M distinct/min. Shrinking constants or adding scheduler caps
  changes the system under check instead of producing a smaller proof of the
  same system; SCOPE-14/15/16 remain provisional without mover checks.
- The 16,670-state verdict applies only to the fault-free, rescan-bounded suffix
  (`faults'=1`, R=8). It cannot be composed into a full-machine claim.
- Small-scope exhaustive search is a falsifier: it finds shallow bugs (the
  round-5 sweep bug) but yields no cutoff theorem for scope N+1, and TLC checks
  one concrete instance, not all instances of that size.
- T2’s 47/47 QNT409 failures are a harness defect (`initP<k>` used as both init
  and step via generated `initAllPinned`), not model evidence.
- Exits from this method family: inductive invariants plus ranking/fairness
  (cost scales with the transition relation, not states), abstraction/refinement,
  or local assume–guarantee models. Further enumeration is predicted waste.

Primary references used for these distinctions: Lamport, _Safety, Liveness, and
Fairness_ (lamport.org/tla/safety-liveness.pdf); Apalache running theory and
bounded-model-checking docs; Quint model-checker and checking-properties docs;
Yu–Manolios–Lamport on TLC; Jackson, _Software Abstractions_ (small-scope
hypothesis).

## Decisions

- 2026-11-15: API calls are environment actors; the system under proof
  is the pipeline. Mutexes/ordering live in action guards, not in
  `step`; branch completeness is machine-checked.
- 2026-11-16: totality guards accepted as two classes (projection
  totalization; resolvability/liveness preconditions mirroring code
  re-checks). `WorkerApplySuccess`/`UploadIndexed` use
  resolvability-conditioned reads mirroring `flush_tree.rs`
  `old_record = None` re-use-or-heal. Album aggregate row id 0.
- 2026-11-17: A-merged single-module model over the generated-copy
  alternative (native Quint instances are copies, not shared bindings —
  QNT500/QNT502; qualified access would need ~2,000 rewrites).
- 2026-11-18: fault model per the re-derivability argument; budgets are
  scoped claim parameters (SCOPE-13), not modeling compromises.
- 2026-11-18: measurement-integrity rule — verifier outcomes are
  classified from output markers, never exit status alone (the 12/12
  artifact).
- 2026-11-19: exploration concluded on the measured evidence above;
  landing decision per the follow-on options.
- 2026-11-20: full-machine bounded lane CLOSED. Harness fix `9ae4c749` landed;
  T1 (2 h) and T2 (47/47 QNT409) produced no machine-checked verdict. No further
  full-machine TLC, T2 smoke, or new scheduler reductions are authorized.
- 2026-11-20: user approved LOCAL PROTOCOL PROOFS. Update the two existing lane
  files only; do not open new plan documents. Local models must carry their own
  instance/budget scope, anti-vacuity witnesses, and an explicit non-composition
  statement. No edits to `Converged`, `AspectsAligned`, or `CompletionInvariant`.
- 2026-11-21: catalog cleanup on `quint/longcat`. Removed G0 and G5 from
  `goals.json` (their content is assumption/code-level: G5 is ASS-3; G0 is
  SCOPE-6 + REQ-13). Merged ASS-2 into ASS-5 (duplicate hash/snapshot premise).
  Removed SCOPE-12 (obsolete `AlbumTaskApply`; album deltas are inline in the
  membership-mutating actions). Corrected stale names in SCOPE-1 (`initialRow`)
  and SCOPE-5 (`RotateImage`). Dropped the closed lane's SCOPE-14/15/16 commits
  from history; the model is back to its pre-cap state. Re-homed REQ-13/15/16 to
  G1. Proof target remains G1.
- 2026-11-21: the active proof plan is `.plan/quint-proof-lane-longcat.md`
  (G1 quiescent convergence via finite divergence-shape abstraction). This file
  remains the exploration record.

## Iteration-1 seeds (for a possible multi-event follow-up)

Substitution rule for k-event pinned prefixes: a substitution map
`state var → its current post expression` applied iteratively — each
event contributes its guards under the current map, then rewrites the
map from its assignment conjuncts. Growth is per-read text, not nesting
depth. Recorded shapes (reduced instance):

- Pair A: `ExternalDelete("p1") → SubmitWatcherRemove(p0)` — guards
  `p0.in(PATHS)`, `not(INIT_files.set("p1", absent).get(p0).present)`;
  assigns `generations' = INIT_generations.set(p0,
INIT_generations.get(p0) + 1)`, `intake' = INIT_intake.union(...)`,
  every other var from its `INIT_<var>`; `files' =
INIT_files.set("p1", absent)`.
- Pair B: `ExternalModify("p1") → ExternalModify(p0)` — nondet
  `cv1/mt1/ln1/sc1/cv2/mt2/ln2/sc2/p0`; `files' =
INIT_files.set("p1", {present: true, cv1, mt1, ln1, sc1}).set(p0,
{present: true, cv2, mt2, ln2, sc2})`; every other var from its
  `INIT_<var>`.

Encoding-feasible, measurement-infeasible while the suffix product
stands (each element re-explores it).
