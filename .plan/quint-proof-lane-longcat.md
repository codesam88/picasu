---
status: open
type: feature
priority: medium
area: testing
---

# Quint proof lane — G1 quiescent convergence

## Problem

The completion machine's bounded exhaustive check did not terminate on the
full instance. The catalog target is G1: after changes stop and a full scan
completes, every asset path has one correct row, no ghosts persist, and
nothing stays queued. The model carries the sweep, lease, identity and
fault-model fixes; the missing piece is a proof shaped to G1 rather than a
full state-space exhaustion.

## Claim and non-claims

Claim, at the reduced instance (3 paths / 2 roots), for any state reachable by
environment events, API calls and at most one budgeted drain fault, once
changes stop and a full scan of both roots completes:

`phase == 2 and scannedRoots == ROOTS and Quiescent` implies
`Converged and AspectsAligned`.

Assumptions: ASS-3 (filesystem-first), ASS-6 (settled-state safety), ASS-7
(a scan trigger runs), SCOPE-6 (crashes occur between completed durable
writes), SCOPE-2 (finite bounds).

Non-claims:

- G2 bounded convergence (needs a ranking argument and ASS-7; separate
  claim).
- Partial-write atomicity (SCOPE-6 lifting; REQ-13 verifies the code).
- The `share_list` DB-only exception (N10).
- Thumbnail content corruption (REQ-8).
- Album aggregate drift: `AlbumStatsConsistent` is a model invariant, not a
  conjunct of `Converged` or `AspectsAligned`; REQ-16 covers it at the code
  level.
- Any instance larger than the completed scale rung.

## Proof architecture

Refinement over a finite shape abstraction, not a monolithic state-space run.

1. **Shape projection.** `shapeOf(s)` maps a concrete state to the set of
   active shape tags. Shapes are the negated conjuncts of the goal
   `Converged and AspectsAligned`.
2. **Covering.** `not(Converged and AspectsAligned)` implies `shapeOf(s)` is
   non-empty. This is definitional if the shapes are the exact negated
   conjuncts; check it with one TLC pass over the goal negation.
3. **Heal.** For each shape tag, the drain+heal suffix reaches a state with
   `phase == 2`, `scannedRoots == ROOTS`, `Quiescent`, `Converged` and
   `AspectsAligned`, from any state satisfying that shape.
4. **Composition.** Covering plus heal gives the claim. The composition is a
   written argument; it needs review, not another verifier run.

If the shape abstraction is coarser than the exact negation (for example,
generation values abstracted to tokens), add a **simulation lemma**: every
concrete action maps a state in shape `Sh` to a state in one of the abstract
successors of `Sh`. The exact-negation shape set needs no simulation lemma;
the generation abstraction does.

## Shape catalog

For `p` in `PATHS`, `p` not in `unprocessable`, `id = assetByPath.get(p)` when
the row exists, and `h = assetById.get(id).contentHash` when the record exists:

| Shape                 | Predicate                                                                                                                   |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `MissingRow(p)`       | `files.get(p).present and not(assetByPath.keys().contains(p))`                                                              |
| `GhostRow(p)`         | `not(files.get(p).present) and assetByPath.keys().contains(p)`                                                              |
| `TornRow(p)`          | `files.get(p).present and assetByPath.keys().contains(p) and not(assetById.keys().contains(id))`                            |
| `WrongPath(p)`        | row and record exist and `assetById.get(id).path != p`                                                                      |
| `StaleContent(p)`     | row and record exist and `assetById.get(id).contentHash != files.get(p).assetHash`                                          |
| `MissingMeta(p)`      | row exists and `not(metadataPayload.keys().contains(id))`                                                                   |
| `StaleMetaHash(p)`    | metadata exists and `metadataPayload.get(id).assetHash != files.get(p).assetHash`                                           |
| `StaleMtime(p)`       | metadata exists and `metadataPayload.get(id).modified != files.get(p).mtime`                                                |
| `StaleSize(p)`        | metadata exists and `metadataPayload.get(id).fileSize != files.get(p).len`                                                  |
| `StaleSidecar(p)`     | metadata exists and `metadataPayload.get(id).sidecar != files.get(p).sidecar`                                               |
| `NotConvergedFlag(p)` | metadata exists and `not(metadataPayload.get(id).converged)`                                                                |
| `MissingThumb(p)`     | row and record exist, `contentHash >= 0`, and (`not(thumbPresent.keys().contains(h))` or `not(thumbPresent.get(h))`)        |
| `MissingDupe(p)`      | row and record exist, `contentHash >= 0`, and (`not(dupeIndex.keys().contains(h))` or `not(dupeIndex.get(h).contains(id))`) |
| `QueuedIntake`        | `intake.size() > 0`                                                                                                         |

`ShapeCatalog(s)` is the disjunction of the per-path shapes over `PATHS` and
`QueuedIntake`. `PendingSweep` and `LeaseHeld` are not goal shapes: they are
part of the `Quiescent` antecedent, not of `Converged`/`AspectsAligned`. The
heal must reach them, and the work measure below includes them.

`Covering(s) = not(Converged and AspectsAligned) implies ShapeCatalog(s)`.

### Reachability audit

Reachable shapes carry the heal obligation:

| Shape                                      | Creating action                                               |
| ------------------------------------------ | ------------------------------------------------------------- |
| `GhostRow`                                 | `ExternalDelete` leaves the path row behind                   |
| `MissingRow`                               | `ApiMoveFsOnly` and the rename windows move files before rows |
| `TornRow`                                  | `RemoveIdOnly` (N5) drops the record before the row           |
| `StaleContent`                             | `ExternalModify` / `ForceModify` change file bytes            |
| `StaleMetaHash`, `StaleMtime`, `StaleSize` | share the `StaleContent` witness                              |
| `StaleSidecar`                             | `ExternalSidecarCreate` changes sidecar bytes                 |
| `MissingThumb`                             | `ThumbLossReboot` / `LoseThumbs`                              |
| `QueuedIntake`                             | `SubmitScan` / `SubmitWatcherIndex`                           |

Unreachable by action inspection, no heal obligation:

| Shape              | Reason                                                                                                                                   |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `MissingMeta`      | no action drops metadata while keeping the row and record; removal actions drop the row too                                              |
| `MissingDupe`      | no action drops a dupe-index entry while keeping the row and record; removal actions drop the row too                                    |
| `WrongPath`        | move and rename rewrite the record path and the path key together; `ApiMoveFsOnly` leaves a missing row, not a record pointing elsewhere |
| `NotConvergedFlag` | the converged flag is only ever written `true`                                                                                           |

The audit is by action inspection. A machine-checked reachability run from
`initCompletion` for each reachable shape is still required by the
anti-vacuity gate.

## Generation ordering

The shapes do not mention `generations`; the goal does not read generation
values, only guards do. Every generation-bumping action is an environment or
request-phase action excluded from `stepHeal`, so generations are constant
during the heal. Two treatments cover the initial value, in order:

1. **Pin and argue.** Pin `generations` to a constant map in every shape init.
   Prove that the heal outcome is independent of the initial generation
   values: the actions that change the goal fields are enabled regardless of
   the pinned values, or the generation guards only drop work that the scan
   rediscovers. If a needed heal action is blocked by a generation guard,
   this treatment fails.
2. **Finite tokens.** Replace `generations` with a finite ordered token set
   and prove no aliasing at saturation. This is the B3 representation route;
   it needs its own soundness witness.

Stop: if the heal proof depends on unbounded generation values and neither
treatment is sound, stop and report. Do not raise a cap and call it a proof.

## Execution spec

### Files

- `utils/quint/proof/shapes.qnt` — shape predicates and `ShapeCatalog`.
- `utils/quint/proof/heal.qnt` — `stepHeal`, shape init actions, work measure.
- `utils/quint/proof/entries/reduced.qnt` — reduced instance binding.
- `utils/quint/proof/entries/canonical.qnt` — canonical instance binding.

These live outside `pipeline/`, so `just quint check` and `just quint test`
are untouched. Add a `verify-proof` recipe to `utils/quint/justfile` that
typechecks the proof modules and runs the heal checks; it is advisory
(nightly), not a merge gate, until G1 lands.

### Module template

`shapes.qnt` imports the reduced instance with the entry-file pattern:

```quint
module proof_shapes {
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3"),
    HASHES = 0.to(1),
    SMALL = 0.to(1),
    SECOND_ROOT_PATHS = Set("p3"),
    SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }],
  ).* from "../pipeline/merged"

  // shape vals and ShapeCatalog here
}
```

`heal.qnt` imports the same instance and defines `stepHeal` over the imported
actions. `stepHeal` includes `BeginDrain`, `BeginScan`, `CompleteScan`,
`WorkerApplySuccess`, `WorkerSkipSuperseded`, `WorkerSkipStale`,
`WorkerSkipUnchanged`, `WorkerOpenFails`, `WorkerRemoveApply`,
`SweepEnumerate`, `SweepRecheck`, `SweepRemoveBatchApply`, `SweepRoot`,
`MoveRowRewrite`, `RotateFinish`, `UploadIndexed`, `IntakePickupFileGone`,
`MemberWrite`, `AlbumShareEdit`, `TrashToggle`, the three `*Reboot` faults,
the literal `RemoveIdOnly` branches, and `Skip`. It excludes environment
actions and untracked `ScanRoot`.

Each shape init is a state predicate plus bookkeeping, for example:

```quint
action initShapeGhostRow = all {
  // bind every state var; use nondet draws for fields the shape leaves free
  files' = ...,
  assetByPath' = ...,
  generations' = PATHS.mapBy(_ => 0),
  phase' = 0,
  scannedRoots' = Set(),
  faults' = 0,
  // ... remaining vars
}
```

The init must cover every state satisfying the shape, not one witness. Use
`nondet` draws for the free fields; the shape predicate constrains the rest.
Copy the concrete assignment pattern from
`utils/quint/local/local-sweep.qnt:initSweepTwoRoot`.

### Commands

```sh
export JRE_BIN="$HOME/.quint/jre/bin"; export PATH="$JRE_BIN:$PATH"
export TMPDIR=/home/codesam/tmp/opencode/proof/tmp
export JAVA_TOOL_OPTIONS="-Djava.io.tmpdir=$TMPDIR"; mkdir -p "$TMPDIR"

quint typecheck utils/quint/proof/heal.qnt --main proof_heal

quint verify utils/quint/proof/heal.qnt --main proof_heal \
  --init initShapeGhostRow --step stepHeal \
  --invariant CompletionInvariant --backend tlc \
  > /tmp/opencode/logs/<date>-heal-ghost.log 2>&1
```

A pass requires `No error has been found` or `[ok] No violation found`,
`> 0 distinct states`, `0 states left on queue`, and a complete-depth line.
Classify from output text, never exit status. Long runs redirect to
`/tmp/opencode/logs/`.

### Anti-vacuity

- **Shape witness:** a deterministic run reaches each reachable shape from
  `initCompletion` (`shapes.qnt`); `StaleContent` uses a TLC counterexample
  because its creating draw can be a no-op.
- **Heal witness:** a run reaches `phase == 2 and scannedRoots == ROOTS and
Quiescent` from each shape.
- **Covering canary:** drop one shape from `ShapeCatalog`; the covering check
  must fail.
- **Heal canary:** remove one heal action from `stepHeal`; at least one
  shape must fail to heal or the run must time out.
- **Generation canary:** if tokens are used, mutate the token bound and
  confirm the verdict changes or the soundness witness fails.

### Gates

`quint typecheck` on the proof modules, `just quint check`, `just quint test`
(70 runs), `just plan-lint`. The proof lane itself is advisory until G1
lands; the 70-run suite remains the merge-gate regression.

## Steps

### 1. Fix the claim and scope (half day)

- Write the claim and assumption list into this plan.
- Decide whether L1 (`utils/quint/local/`) stays as a regression canary or is
  removed. Default: keep.
- No model or property edits.

Gate: reviewed claim and scope.

### 2. Shape catalog (1–2 days)

- Add `utils/quint/proof/shapes.qnt` with the predicates above.
- Add `ShapeCatalog` and `Covering`.
- Add one witness run per shape.

Gate: typecheck; covering check passes; every reachable shape has a
reachability witness; unreachable shapes are listed with a reason.

Stop: a shape that cannot be expressed as a finite predicate over the model
state.

### 3. Covering check (half day)

- `quint verify` with `--invariant Covering` over a init that draws arbitrary
  states (or over `stepCompletion` from `initCompletion` at the reduced
  bounds).
- Mutation canary: remove one shape; the check must fail.

Gate: terminating pass with the anti-vacuity markers.

### 4. Heal proof (3–5 days)

- Add `heal.qnt` with `stepHeal`, the shape inits, and the work measure.
- For each reachable shape, run the heal check and record states, depth,
  runtime.
- If TLC does not terminate, apply the generation treatment above.

Gate: every shape terminates with a positive verdict and a heal witness.

Stop: a shape is not healed, or the generation treatment is unsound.

### 5. Compose G1 — written argument (scoped to the drawn families)

**Theorem (G1, scoped).** On the reduced instance (3 paths / 2 roots), for
every state `s` in the drawn families `F` (the seven witness inits, the
invariant-constrained ghost family, and the combination family), if a state
reachable from `s` by `stepHeal` satisfies `phase == 2 and
scannedRoots == ROOTS and Quiescent`, then `Converged and AspectsAligned`.

**Machine-checked evidence** (all in-repo, `utils/quint/proof/proof.qnt`):

1. **Lemma (covering).** `Covering` — every goal-violating state satisfies at
   least one catalog shape — holds over `F`'s initial states (16,856 + 12)
   and exhaustively over the 190,420-state graph reachable under `stepHeal`.
   Shape soundness (`ShapeSound`) holds: no catalog shape occurs in a
   goal-true state. The shapes are the negated conjuncts of
   `Converged and AspectsAligned` by construction.
2. **Lemma (heal).** `CompletionInvariant` holds over the graph reachable
   from each family init under `stepHeal`, exhaustively (queue 0, complete
   depth 10–11, 190,420 states for the combination family).
3. **Non-vacuity.** `HealAntecedentReached` counterexamples for every family
   init: the antecedent is reached from each.
4. **Reachability of the families.** The family inits satisfy
   `ReachableInv` by construction, and `ReachableInv` holds at
   `initCompletion` (falsification-checked over 3.99M states; not inductive).
   Per-shape reachability witnesses: six deterministic runs and the
   `StaleContentReached` counterexample.

**Proof (scoped).** Take any state `t` reachable from `s` by `stepHeal` with
the antecedent holding. If `t` satisfies the goal, done. Otherwise, by the
covering lemma applied along the graph (checked over the full `stepHeal`
graph, not just the initial states), `t` satisfies a catalog shape; each
catalog shape is a goal violation (soundness), and the heal lemma applied to
that shape's family gives a heal trace to the goal — but `t` is terminal for
the antecedent, so the covering lemma's exhaustiveness over the graph is the
operative fact: no goal-violating state exists in the checked graph.

**Honest scope — what this does not yet prove:**

- `F` is a subset of the reachable closure characterized by the drawn
  dimensions (per-path kinds, thumbnail, sweep-pending, stale dupe, intake,
  lease windows). It is not the full reachable set; universal-over-the-shape
  needs full field draws.
- `ReachableInv` is falsification-checked, not proved inductive.
- ASS-3/6/7 and SCOPE-2/6/13 remain assumptions; the model↔code link is out
  of scope.
- The full G1 sentence over all reachable states follows only with (a) an
  inductive `ReachableInv` and (b) universal shape families — or the
  inductive route directly.

**Quotable claim (scoped).** "On the reduced instance (3 paths / 2 roots),
for every state in the drawn families `F` — a subset of the reachable closure
characterized by `ReachableInv` — TLC exhausts the `stepHeal` graph with no
violation of `CompletionInvariant`, and every goal-violating state in `F`
satisfies a catalog shape that is itself a goal violation."

- Independent review of the shape predicates, the covering check, the heal
  init coverage, and the composition: pending (see the review record below).

Gate: reviewer sign-off; claim quoted with instance, bounds and assumptions.

### 6. Scale ladder

- Run the heal check at 3 paths / 2 roots, then 4, then canonical 5.
- Record states, depth and runtime per rung; stop at ~100k states or ~10
  minutes.
- The covering lemma is parameterized; the heal check is per instance. Quote
  the largest completed rung.

Gate: the largest completed rung is recorded; no claim beyond it.

## Stop/go

- No edits to `Converged`, `AspectsAligned`, or `CompletionInvariant`.
- Stop a rung at ~100k states or ~10 minutes; narrow the slice, do not raise
  budgets.
- If covering, heal and the inductive-core fallback all stall, stop. Keep
  the 70-run suite and L1, and move the remaining risk to production
  monitors.
- Fallback: if the heal proof stalls on generation handling, extract the
  drain+heal core with total maps and finite epoch tokens (the inductive-core
  route) and prove the goal there. If that also stalls, stop.

## Risks

- The shape projection may be too coarse: two concrete states in one shape
  may heal differently. Refine the shape or add a simulation lemma.
- Generation ordering may block the heal proof; the token abstraction may be
  unsound at saturation.
- Composition is the most likely place for a hidden gap; it needs review.
- Scaling may be expensive even with the abstract proof.

## Learnings

- 2026-11-21: `MissingMeta`, `MissingDupe`, `WrongPath` and `NotConvergedFlag`
  are unreachable by action inspection. The `MissingDupe` heal counterexample
  was vacuous: the heal step does not repair an incomplete dupe index, but no
  reachable state has one. Removal actions drop the row and the dupe entry
  together.
- 2026-11-21: the seven reachable shapes heal from witness inits, and the
  heal antecedent is reachable in each. The witness inits are single states,
  not universal shape inits.
- 2026-11-21: reachability from `initCompletion` is machine-checked for the
  seven shapes (six deterministic runs, one TLC counterexample for
  `StaleContent`).
- 2026-11-21: the semi-universal ghost-row init (2304 drawn states) found a
  heal counterexample, but the drawn state is unreachable from
  `initCompletion`: it has `dupeIndex[0] = {2}` while `p3`'s row and record
  are present, and no action removes a dupe entry while keeping the row. The
  mechanism is real: `WorkerSkipUnchanged` does not test the dupe index, so an
  incomplete bucket survives to quiescence. Learning: universal shape inits
  must be constrained by the model's reachability invariants; drawing
  arbitrary states inside a shape yields unreachable states and vacuous
  falsifications. `DupeComplete`, `MetadataComplete`, `PathConsistent` and
  `ConvergedFlagSet` characterize the reachable closure and are the bridge to
  the deferred inductive track.
- 2026-11-21: with a singleton `leased` draw, `stepHeal` has no lease release
  and `BeginScan` requires quiescence, so 1728 of 2304 drawn states satisfy
  the invariant vacuously. The heal step needs a lease-release path, or the
  lease dimension must be excluded from the heal family.
- 2026-11-21: the reachability invariants collapse the ghost family. Under
  `PathConsistent` the ghost row's id must be its own `initialId` (no
  collisions), `MetadataComplete` couples record and metadata presence, and
  `DupeComplete` pins the dupe bucket to the existing record ids. The
  2304-state drawn family becomes 12 states. The reachable closure is highly
  constrained; that strength is what makes the constrained family tractable.
- 2026-11-21: falsification check for `ReachableInv` (positive polarity,
  counterexample = a reachable state violating it): 120 s, 2.59M distinct
  states explored, no counterexample; frontier still growing, so not
  exhaustive and not inductive. Polarity note: `--invariant ReachableInv` is
  the falsification form; a `ReachableInvNeg` counterexample at init is a
  witness that init satisfies the invariants.
- 2026-11-21: the invariant-constrained ghost family heals completely: 12
  initial states, 346 distinct states, queue 0, depth 10, no violation, and
  the antecedent is reached from every drawn state. This is a per-family
  check over the ghost path's own structure; other paths are pinned to base,
  so it is not yet universal over the shape.
- 2026-11-22: the cross-shape combination family heals completely. 728
  initial states (per-path divergence kinds over
  ok/stale/staleSidecar/ghost/missingRow/torn with at least one ghost, plus
  the thumbnail flag and the sweep-pending set) exhaust at 13,780 distinct
  states, queue 0, depth 10, with no `CompletionInvariant` violation, and the
  antecedent is reachable (the all-ghost trace reaches quiescence through
  sweep alone). Cross-shape interactions do not break the heal at the reduced
  instance. Pinned dimensions, logged in the init: stale dupe entries, intake,
  leases, unprocessable.
- 2026-11-22: the combination family with stale dupe entries and one queued
  intake item exhausts at 83,896 distinct states (8,792 distinct initial
  states), queue 0, depth 10, with no `CompletionInvariant` violation. Stale
  dupe entries (an id in the bucket without a record, reachable via
  `RemoveIdOnly`) are tolerated: `AspectAligned` requires entries for rows
  with records, not the absence of extras. A queued intake item with the
  current snapshot is drainable in one step for every file state (present →
  apply, absent + watcher → open-fails, absent + remove → remove-apply). The
  stale-dupe and intake dimensions are now drawn; leases and unprocessable
  remain pinned. Measurement detail: the raw draw product (34,944) collapses
  to 8,792 distinct initial states because the derived dupe bucket repeats
  across kind triples.
- 2026-11-22: the lease-window dimension heals. The combination family with
  move and rotate windows (16,856 distinct initial states) exhausts at
  190,420 distinct states, queue 0, depth 11, with no `CompletionInvariant`
  violation. Every drawn move window is released by `MoveRowRewrite` and every
  rotate window by `RotateFinish`; their guards depend only on the drawn
  fields, so no combination blocks a release. Single-state witness runs for
  rotate, move, and move+rotate each exhaust with the antecedent reached
  (49/29/37 distinct states). No REQ-7 or REQ-11 falsification.
- 2026-11-22: limits of the lease-window draw: move windows are drawn from
  base bytes only (a move of an already-stale file is not drawn);
  generations are pinned at 0, so the absence of a generation guard in
  `MoveRowRewrite` is untested; rotate windows are first-rotation only
  (sidecar 0 to 1, not 1 to 2); unprocessable stays pinned.
- 2026-11-22: toolchain artifact — TLC stalls in violation reporting on the
  large witness run (main blocked in `ModelChecker.runTLCContinueDoing`,
  workers in `Worker.doNextSetErr`); the witness verdict is read from the
  printed trace. Reproduced 3/3. The single-state witness runs are the
  durable machine-checked witness evidence.
- 2026-11-22: the covering lemma holds over the drawn families and their
  reachable graph. `Covering` passes over the combination family's 16,856
  initial states, over the ghost family's 12, and exhaustively over the
  190,420-state graph under `stepHeal` (queue 0, depth 11). Soundness holds:
  no catalog shape occurs in a goal-true state (`ShapeOverBroad` empty over
  16,856 states). Every goal-violating drawn state is catalogued, and the
  graph preserves covering to completion.
- 2026-11-22: polarity rule (learned the hard way): `quint verify --invariant
X` checks that X holds; a violation means X was false. Positive-form
  invariants for properties that must hold (`Covering`, `not ShapeOverBroad`);
  negation only for witness-by-counterexample (reachability). The first
  covering brief had the polarity inverted; the runs were corrected before
  any verdict was recorded.
- 2026-11-22: structural constraint — Quint instances are per import site.
  The catalog (`shapes.qnt`) and the heal inits (`heal.qnt`) cannot be
  combined into one TLC spec: importing one into the other instantiates
  `pipeline_merged` twice (distinct variable sets), and `export` flattens
  before instances and loses the constant bindings. The covering checks ran
  on a generated harness under `/tmp/opencode/cover-harness/` (derived
  artifact, not committed). Making the checks in-repo requires restructuring:
  a single merged proof module, or an action-free shared base that declares
  the state once (the `local_common` pattern).
- Open: the covering check ran on a generated harness, not in-repo (see the
  structural constraint above). The heal evidence is per-family, not
  per-shape-universal via full field draws.

## Status

- `utils/quint/proof/proof.qnt`: the single shape-proof module (merged from
  shapes.qnt and heal.qnt): shape catalog, `ShapeCatalog`,
  `Covering`/`CoveringNeg`/`ShapeOverBroad`/`ShapeSound`, the reachability
  invariants, six reachability runs, the `StaleContentReached` counterexample,
  `stepHeal`, the seven witness inits, the ghost family, and the combination
  family. The full check set runs in-repo on this one spec.
- Check results: nine heal checks and antecedent witnesses green (combination
  family 190,420 distinct states, queue 0, depth 11); `Covering` and
  `ShapeSound` green over both families and the graph; six reachability runs
  pass; `StaleContentReached` counterexample at depth 4; `ReachableInv`
  falsification timeout at 3.99M states, no counterexample.
- Reachability witnesses are machine-checked for the seven reachable shapes;
  the four unreachable shapes are excluded by action inspection, and
  `ReachableInv` characterizes them (falsification-checked, not yet
  inductive).
- `quint test utils/quint/proof/proof.qnt --match='Reachable'` runs the proof
  runs; the name filter also selects `ReachableInv`/`ReachableInvNeg`, which
  fail as tests (pre-existing), and the imported model runs reference
  out-of-binding paths.
- Remaining: reviewer sign-off on the composition; unprocessable paths,
  stale-file move windows, the generation dimension of the move rewrite,
  universal-over-the-shape via full field draws, inductive proof of
  `ReachableInv` (deferred).
- The inductive full-model proof is deferred; the shape artifacts feed later
  invariant discovery.

## Decisions

- 2026-11-21: target G1 on `quint/longcat`; `merged.qnt` is the source of
  truth. G2 is out of scope; atomicity stays assumed (SCOPE-6 + REQ-13).
  The active proof plan is this file; `.plan/quint-proof-lane.md` is the
  exploration record.
- 2026-11-21: shape decomposition continues as discovery and falsification;
  the inductive full-model proof is deferred. The shape artifacts are inputs
  to later invariant discovery, not the final proof architecture.
