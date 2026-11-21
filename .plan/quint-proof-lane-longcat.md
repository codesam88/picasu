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

## Claim

On the reduced instance (3 paths / 2 roots), for any state reachable by
environment events, API calls and at most one budgeted drain fault, once
changes stop and a full scan of both roots completes:

`phase == 2 and scannedRoots == ROOTS and Quiescent` implies
`Converged and AspectsAligned`.

Assumptions: ASS-3 (filesystem-first), ASS-6 (settled-state safety), ASS-7
(a scan trigger runs), SCOPE-6 (crashes occur between completed durable
writes), SCOPE-2 (finite bounds).

Out of scope:

- G2 bounded convergence (needs a ranking argument and ASS-7; separate
  claim).
- Partial-write atomicity (SCOPE-6 lifting; REQ-13 verifies the code).
- The `share_list` DB-only exception (N10).
- Thumbnail content corruption (REQ-8).

## Approach

Finite divergence-shape abstraction with heal closure.

1. **Shape catalog.** Enumerate every way `Converged`/`AspectsAligned` can
   be false. For each shape: violating predicate, creating environment
   actions, healing pipeline action, witness state.
2. **Closure.** Every environment action maps a shape to a shape or
   preserves convergence.
3. **Heal and termination.** The drain+heal suffix maps each shape to
   converged quiescence; a work measure combining intake, sweep candidates,
   leases, and unscanned roots decreases or progresses.
4. **Composition.** Combine closure and heal into the G1 sentence.
5. **Scale.** 3 paths / 2 roots → 4 → canonical 5; quote the largest
   completed rung.

The fault-free drain+heal suffix is already small (16,670 states, depth 29,
36 s), so the proof object is the suffix plus a finite abstraction of the
request-phase residue, not the unbounded request phase.

## Steps

### 1. Fix the claim and scope (half day)

- Write the claim and assumption list into this plan.
- Decide whether L1 (`utils/quint/local/`) stays as a regression canary or
  is removed. Default: keep.
- No model or property edits.

Gate: reviewed claim and scope.

### 2. Divergence-shape catalog (1–2 days)

- Add `utils/quint/proof/shapes.qnt` (outside `pipeline/`, so the existing
  gates are untouched).
- One `val` per shape plus a `ShapeCatalog` invariant.
- Cover at least: missing row, ghost row, stale metadata, wrong sidecar
  identity, missing thumbnail, queued intake, pending sweep, held lease,
  torn N5 residue, unindexed upload, unprocessable path.
- For each shape record the creating actions and the healing action.

Gate: typecheck; every shape is reachable in a witness run; every shape has
a healing action.

Stop: a shape that cannot be expressed as a finite predicate over the model
state.

### 3. Closure proof (2–4 days)

- Inductive invariant: environment actions preserve the shape set.
- Check on the abstract shape module with TLC (or Apalache if the module is
  symbolic).
- Mutation canary: remove one shape; closure must fail.

Gate: terminating check with a positive verdict, positive state count,
drained queue, complete depth.

Stop: the shape set is not closed; fall back to the inductive core.

### 4. Heal and termination (3–5 days)

- Start from each shape; run the drain+heal suffix to quiescence.
- Define the work measure and show it decreases or progresses.
- Mutation canary: remove one heal action; at least one shape must remain
  unrepaired.

Gate: terminating check with a positive verdict and the anti-vacuity
protocol.

Stop: a shape is not healed; narrow the slice or fall back.

### 5. Compose G1 (1–2 days)

- Write the refinement/assume–guarantee argument that combines closure and
  heal into the claim.
- Independent review of the composition.

Gate: reviewer sign-off; claim quoted with instance, bounds and assumptions.

### 6. Scale ladder

- 3 paths / 2 roots → 4 → canonical 5.
- Each rung is a measured decision; stop at the agreed state/time budget.

Gate: the largest completed rung is recorded; no claim beyond it.

## Verification and anti-vacuity

- Classify verifier outcomes from output markers, never exit status.
- A pass requires a positive verdict marker, > 0 distinct states, a drained
  queue, and a complete-depth line.
- Witness each shape and the quiescence antecedent.
- Mutation canaries for the shape set, the heal actions, and any caps.
- `quint typecheck`, `just quint check`, `just quint test`,
  `just plan-lint`.

## Stop/go

- No edits to `Converged`, `AspectsAligned`, or `CompletionInvariant`.
- Stop a rung at ~100k states or ~10 minutes; narrow the slice, do not raise
  budgets.
- If closure and the inductive-core fallback both stall, stop. Keep the
  70-run suite and L1, and move the remaining risk to production monitors.

## Risks

- The shape set may not be finite because generation ordering is unbounded;
  finite epoch tokens or an ordering abstraction may be required.
- Closure may fail; the fallback is the inductive core on the drain+heal
  suffix.
- Composition is the most likely place for a hidden gap; it needs review.
- Scaling may be expensive even with the abstract proof.

## Decisions

- 2026-11-21: target G1 on `quint/longcat`; `merged.qnt` is the source of
  truth. G2 is out of scope; atomicity stays assumed (SCOPE-6 + REQ-13).
  The active proof plan is this file; `.plan/quint-proof-lane.md` is the
  exploration record.
