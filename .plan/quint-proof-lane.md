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
