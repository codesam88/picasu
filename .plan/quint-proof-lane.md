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
2. Invariant re-expression: `val Settled = ...` (quiescence conditions) and
   `val inv = Settled implies (Converged and AspectsAligned)`. Design risk:
   quiescence alone is insufficient — the FS_MODIFY_INFLIGHT gap pins
   permanent staleness at a settle point (`skipStaleTest`), and G1 requires
   "changes stopped AND a full scan completed". Spike path: check with a
   plain quiescence `Settled` first; the violation shapes the checker
   reports are expected to be exactly the documented open-fault cases
   (S6-style staleness). Then decide between (a) an additive per-root
   scan-completion watermark variable or (b) a documented-stale-shapes
   disjunction; (a) is the lean.
   Status: step module landed (248 branches, base + 16 aspects actions ×
   parameter ranges + Skip); `Settled`/`ConvergenceInvariant` defined;
   sanity runs pass at 10k after totality guards.
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
6. Run on a nightly lane only; `quint run` stays the merge gate. Claims quote
   the SCOPE register and the step bound.
7. Deferred: eventual-consistency full-model bake-off, inductive invariants,
   liveness/temporal properties.

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
