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

- **Shape witness:** a deterministic run reaches each shape from
  `initCompletion` through environment actions.
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

Gate: typecheck; covering check passes; every shape has a reachability
witness.

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
- For each shape, run the heal check and record states, depth, runtime.
- If TLC does not terminate, apply the generation treatment above.

Gate: every shape terminates with a positive verdict and a heal witness.

Stop: a shape is not healed, or the generation treatment is unsound.

### 5. Compose G1 (1–2 days)

- Write the covering-plus-heal argument that yields the claim, in this shape:
  1. Lemma (covering): a state violating `Converged and AspectsAligned`
     satisfies at least one shape.
  2. Lemma (heal): from any state satisfying a shape, the drain+heal suffix
     reaches `phase == 2 and scannedRoots == ROOTS and Quiescent` with
     `Converged and AspectsAligned`.
  3. Theorem: the claim.
  4. Proof: take any reachable state. If it satisfies the goal, done;
     otherwise Lemma 1 gives a shape, and Lemma 2 gives a concrete heal trace
     to the goal.
- Independent review of the shape predicates, the covering check, the heal
  init coverage, and the composition.

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

## Decisions

- 2026-11-21: target G1 on `quint/longcat`; `merged.qnt` is the source of
  truth. G2 is out of scope; atomicity stays assumed (SCOPE-6 + REQ-13).
  The active proof plan is this file; `.plan/quint-proof-lane.md` is the
  exploration record.
