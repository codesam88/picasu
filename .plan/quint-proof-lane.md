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
modeled bounds. The specs also only carry explicit `.then(...)` scenario
chains, which neither `quint verify` nor trace generation can consume —
both need `init` plus a nondeterministic `step`.

## Goal

A nightly proof lane that model-checks the Quint models with `quint verify`,
on the asset-aspects slice first, with backend choice decided by measured
data rather than assumption. Safety-only: settled-state invariants
(`settled => Converged and AspectsAligned`), no liveness/fairness (ASS-6,
SCOPE-3). Bounded model checking is the intended stop; inductive invariants
only if the data shows the unbounded claim is actually needed.

## Implementation steps

1. Write the nondeterministic `step = one of { ... }` for the asset-aspects
   module (every branch assigns every state variable — existing discipline).
   The `step` introduces scheduler nondeterminism the scenario runs avoided:
   add a scope.json entry bounding it ("any action may fire at each step, no
   fairness") so the verification claim stays honest per the scope register.
2. Re-express the run assertions as invariant definitions
   (`settled => Converged and AspectsAligned`), checked after every step.
3. Backend bake-off, measured per slice: Apalache (default backend;
   measure the required `--max-steps` — crash-heal chains likely exceed the
   default 10; JRE 17+/21 and version pinning are CI prerequisites) vs
   `quint verify --backend=tlc` (explicit-state; expected to explode on the
   full intake-set universe — one measurement on reduced bounds to confirm).
4. Run on a nightly lane only; `quint run` stays the merge gate. Claims quote
   the SCOPE register and the step bound.
5. Deferred: eventual-consistency slice (larger action set), inductive
   invariants, liveness/temporal properties.

## Decisions

- 2026-11-15: scope set after reading quint.sh checking-properties /
  model-checkers docs and measuring tool state (quint 0.33.1 ships
  `--backend=tlc`; Apalache is actively maintained under apalache-mc, not
  deprecated). Brief: `docs/briefs/quint-modeling.md`.
- 2026-11-15: spike work happens in the `invest/quint-mbt` worktree;
  mainline merge only after the backend data is in.
