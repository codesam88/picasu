---
status: open
type: feature
priority: medium
area: testing
---

# Inductive proof of the convergence claim

## Notes

Execution-ready plan for an inductive proof of `CompletionInvariant` on the
`quint/longcat` track (HEAD `a9df08b0`). Audience: an engineer (human or AI)
executing without deep Quint/Apalache experience. Every step has an exact
command and a binary pass/fail/stop criterion. If a criterion is not met,
stop and report — do not improvise.

Parent records: `.plan/quint-proof-lane-longcat.md` (shape track, §5
composition, Learnings, Status) and `.plan/quint-proof-lane.md` (exploration
5 inductive spike, measurement summary). Model:
`utils/quint/pipeline/merged.qnt` (`pipeline_merged`). Shape artifacts:
`utils/quint/proof/proof.qnt`. Scope/goals:
`utils/quint/pipeline/scope.json`, `goals.json`.

## 1. Claim and non-claims

### Exact theorem

On the reduced instance (PATHS `p1..p3`, `SECOND_ROOT_PATHS = {"p3"}`,
`HASHES = 0..1`, `SMALL = 0..1`, `SUBTREE_PAIRS = [{src: "p1", dst: "p3"}]`),
with the assumptions below, there exists a state predicate `I` such that:

1. `initCompletion => I` (initiation),
2. `I /\ stepCompletion => I'` (consecution, Apalache `--inductive-invariant`),
3. `I => (phase == 2 and scannedRoots == ROOTS and Quiescent implies Converged and AspectsAligned)` (goal implication, i.e. `I => CompletionInvariant`).

`stepCompletion` (`merged.qnt`, `@step-universe` list) is the full phase
machine: phase-0 environment + pipeline actions, phase-1 drain with the
budgeted fault (`faults < 1`, reboot to phase 0), phase-2 tracked
`CompleteScan` per root. The phase machine and the ASS-7 scan trigger are the
mechanism; the proof is a **safety invariant**, not a liveness proof. No
fairness premise is asserted anywhere.

### Assumption list (statuses per registry; reconciled in A5, §4.0)

- ASS-3 (filesystem first; registry: `held`), ASS-6 (settled-state safety;
  registry: `open`), ASS-7 (backstop scan trigger runs; registry:
  `unverified`).
- SCOPE-2 (finite bounds), SCOPE-6 (crashes between completed durable
  writes), SCOPE-13 (nondeterministic scheduling; phases encode the
  observation premise). New entries created by this plan: SCOPE-17
  (smallest-free-id quotient, §2.1), SCOPE-18 (generation-token soundness,
  §2.3).
- A5 (§4.0) reconciles this list with `assumptions.json`: statuses are
  updated where gate evidence exists, otherwise carried verbatim. The
  quoted claim (§6) states each ASS status exactly as the registry has it
  at claim time — the plan must not say "settled" where the registry
  disagrees.

### Explicit non-claims

- Liveness/fairness (the triggers run; the scan completes). Out.
- G2 bounded convergence (needs a ranking argument; separate claim). Out.
- Atomicity/partial writes (SCOPE-6 lifting; REQ-13 verifies the code). Out.
- Model↔code link. **Non-goal of this plan**: no code mapping, no
  refinement claim against `backend/`, no traceability edits.
- Any instance larger than the completed rung. If the inductive checks pass
  at reduced bounds, re-running them at canonical bounds is a recorded
  follow-up rung, not part of the quoted claim.

### No-touch rule

`Converged`, `AspectsAligned`, `CompletionInvariant` in `merged.qnt` are
untouchable. All redesign work happens in the new files under
`utils/quint/inductive/` (§7); no edits to `utils/quint/pipeline/` during
Phases 1–4. The one exception is decided in A3 (§4.0): a model finding from
the album-row disposition lands as a fidelity fix in `merged.qnt` first
(own commit, guard additions mirroring the code's album-row rebuild), the
core is re-copied from the fixed model, and the fix never touches the three
untouchable predicates.

### Result chain

The triple proves a property of `inductive-core.qnt`, not of the backend.
The chain to code is: core ≈ merged (generated rewrite + 70-run equality +
the §4.5 differential) and merged ↔ code (the existing traceability and REQ
gates; the MBT lane is a separate plan). The quotable claim (§6) states
this chain; the model↔code link itself stays out of scope (non-claim
above).

## 2. Representation redesign (prerequisite, blocks everything)

The 2026-11-19 spike (`0b7de1ff`, `.plan/quint-proof-lane.md` exploration 5)
showed Apalache's inductive check rejects the model: 8 of 16 state vars have
no domain expression. Apalache accepts maps only as **total maps over a
constant finite key set with a finite value set**. The redesign makes every
var total over constant finite sets. The new model file is a copy of
`merged.qnt` rewritten table by table (see §7 for paths).

### 2.1 Target representation (exact)

Constants (same names/values as the reduced instance): `PATHS`, `ROOTS`,
`HASHES`, `SMALL`, plus `IDS = 0.to(PATHS.size() + 1)` and epoch bound `G`
(derived from the A6 probe, §2.3; the raise rule allows exactly one bump).

| Var               | Now (unencodable)                     | Redesign (total, finite)                                                                                                                                                                                                                                                                                                                                     |
| ----------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `files`           | total map, no domain expr             | total map `PATHS -> FileState`; add domain conjunct `files.keys() == PATHS` first in `I` (§3). Record fields already finite (`assetHash` in `HASHES`, rest in `SMALL`).                                                                                                                                                                                      |
| `generations`     | `str -> int`, unbounded               | total map `PATHS -> 0..G`; every `+ 1` bump becomes `satSucc` (§2.3)                                                                                                                                                                                                                                                                                         |
| `assetByPath`     | sparse `str -> int`                   | total map `PATHS -> int`, range `IDS`, sentinel `0` = no row (id 0 is the album row, never a path row). Presence test `assetByPath.keys().contains(p)` becomes `assetByPath.get(p) != 0`; read `assetByPath.get(p)` is guarded by `!= 0`                                                                                                                     |
| `assetById`       | sparse `int -> AssetRecord`           | total map `IDS -> AssetRecord`, sentinel `kind == "absent"`. Presence test becomes `assetById.get(id).kind != "absent"`; key-coherence (`AssetIdKeyCoherent`) becomes `kind == "absent" or assetId == key`                                                                                                                                                   |
| `metadataPayload` | sparse `int -> MetadataRecord`        | total map `IDS -> MetadataRecord`, sentinel `kind == "absent"`; same presence rewrite                                                                                                                                                                                                                                                                        |
| `dupeIndex`       | sparse `int -> Set[int]`              | first try: total map `HASHES -> Set[int]` with conjunct bounding members to `IDS`. Test point (§2.4) decides; fallback: boolean matrix `HASHES -> (IDS -> bool)` (two nested total maps, no set values)                                                                                                                                                      |
| `thumbPresent`    | sparse `int -> bool`                  | total map `HASHES -> bool` (`HASHES.mapBy(h => ...)` at init; `Map()` becomes all-false)                                                                                                                                                                                                                                                                     |
| `sweepBatch`      | sparse `str -> int`                   | total map `PATHS -> int`, range `-1..G`, sentinel `-1` = no entry. `sweepBatch.keys().contains(p)` becomes `sweepBatch.get(p) != -1`; emptiness in `Quiescent` becomes `PATHS.forall(p => sweepBatch.get(p) == -1)`                                                                                                                                          |
| `sweepPending`    | already total `ROOTS -> bool`         | unchanged; add domain conjunct `sweepPending.keys() == ROOTS`                                                                                                                                                                                                                                                                                                |
| `intake`          | set of records, unbounded generations | subset of the finite universe `val IntakeUniverse =` the cross product `PATHS x 0..G x {"watcher","remove","scan"} x SMALL x SMALL x SMALL x HASHES`, with conjunct `intake.forall(i => IntakeUniverse.contains(i))`                                                                                                                                         |
| `nextAssetId`     | unbounded counter                     | eliminated. Allocation reads `nextFreeId`, a derived `def` over the redesigned `assetById`: the smallest id in `IDS \ {0}` whose record is absent. No var, no reset (`DbLoss` reset becomes vacuous). No-exhaustion is discharged by the `IdBudget` conjunct (§3.2 item 2). Recorded as SCOPE-17: smallest-free-id quotient; lifting step: counter semantics |
| `leasedPaths`     | `Set[str]`                            | add conjunct `leasedPaths.subset(PATHS)`-equivalent (`leasedPaths.forall(p => p.in(PATHS))`)                                                                                                                                                                                                                                                                 |
| `unprocessable`   | `Set[str]`                            | same subset conjunct over `PATHS`                                                                                                                                                                                                                                                                                                                            |
| `phase`           | `int`                                 | conjunct `phase.in(0.to(2))`                                                                                                                                                                                                                                                                                                                                 |
| `scannedRoots`    | `Set[str]`                            | conjunct `scannedRoots.subset(ROOTS)`-equivalent (`forall(r => r.in(ROOTS))`)                                                                                                                                                                                                                                                                                |
| `faults`          | `int`                                 | conjunct `faults.in(0.to(1))` (K=1)                                                                                                                                                                                                                                                                                                                          |

Album/thumbnail notes: the album row (id 0) keeps its fixed records
(`initialAlbumAssetRecord`, `initialAlbumMetadataRecord`); thumbnail absence
is `thumbPresent.get(h) == false`, never a missing key.

Apalache form (required): every "total map" above must be expressed in
`setOfMaps(KEYS, VALUES)` form in the domain conjuncts (§3.2 item 1),
because Apalache's inductive rewriter accepts maps only in that form (the
2026-11-19 spike's encodability notes). Worked example:
`assetByPath.in(setOfMaps(PATHS, IDS))` for the domain, with presence as
`assetByPath.get(p) != 0`. An arbitrary conjunct over a sparse map is not
accepted.

### 2.2 Mechanical rewrite scope (~680 action-body mentions)

Reproduce the baseline counts first (binary reference):

```sh
for v in assetByPath metadataPayload assetById dupeIndex sweepBatch thumbPresent files generations intake; do printf '%s ' "$v"; grep -c "$v" utils/quint/inductive/inductive-core.qnt; done
```

Expected baseline (from the spike, ± drift): `assetByPath` ~252,
`metadataPayload` ~147, `assetById` ~123, `dupeIndex` ~60, `sweepBatch` ~52,
`thumbPresent` ~44; remainder is `files`/`generations` reads. Rewrite rule
per table: introduce one `pure def` accessor pair (e.g. `apHas(p)`,
`apGet(p)`), then replace every body mention mechanically; never change a
guard's meaning (presence `!=` sentinel exactly mirrors `keys().contains`).
One table per commit, in this order: `thumbPresent`, `sweepBatch`,
`assetByPath`, `assetById`, `metadataPayload`, `dupeIndex`,
`files`+`generations`+`intake`+`nextFreeId` (last: touches the epoch design
and the id quotient).

Iteration sites (the second class of mentions, easy to miss): every
`.keys()` fold, `.keys().mapBy`, `.keys().exclude`, and `Map()` initializer
must become total-domain iteration (`PATHS.fold`, `IDS.mapBy`) or an
all-false/absent initializer, not a sparse-key read. Enumerate them first:
`grep -n "\.keys()" utils/quint/inductive/inductive-core.qnt` and rewrite
each one; the count falls to zero only where the site is a presence test
rewritten per the rule above. The `rows` projection
(`merged.qnt:392-402`) is one such site — rewrite it with the tables it
reads.

Drift control (standing rule): `inductive-core.qnt` is a derived artifact of
`merged.qnt`, produced by per-table codemod scripts committed under
`utils/quint/inductive/tools/` (one script per slice: accessor-pair
substitution plus the `.keys()`-site rewrite; the prefix-generator is the
in-repo precedent). The committed core file is script output; the drift
check is regenerate-and-diff — re-run the scripts, `git diff` must be
empty. All model changes land in `merged.qnt` first; the scripts are then
re-run. Never edit `inductive-core.qnt` to fix a model behavior: that fixes
the copy, not the model, and is exactly the green-proof-of-the-wrong-system
failure.

Beyond the 70-run suite, the core↔merged bridge carries one standing
semantic check: the paired forward-simulation differential (§4.5), run once
after Phase 2 and re-run after any later change to either model. The suite
checks the runs' assertions; the differential checks the behavior delta the
rewrite must not introduce.

### 2.3 Finite epoch tokens for `generations`

- `pure def satSucc(g: int): int = if (g < G) g + 1 else g` (define once;
  `G` is a `const`).
- Replace every `generations.get(x) + 1` bump with `satSucc(...)`. Enumerate
  bump sites (do not guess from old line numbers):
  `grep -n "generations.get(.*) + 1" utils/quint/inductive/inductive-core.qnt` —
  after the rewrite this grep must return nothing (binary criterion).
- The 11 comparison sites keep their text (`==` on tokens; `<` for
  supersede; `sweepBatch.get(p) == generations.get(p)`; `NoLostUpload`
  ordering reads): enumerate with
  `grep -n "it.generation\|generation ==\|generation <\|SweepRecheckBatchClean\|NoLostUpload"`.
- Soundness argument (the one new proof obligation), two parts:
  - **Polarity.** Every comparison site is `==` (freshness) or `<`
    (superseded/stale) and tokens only move forward (`satSucc` is monotone;
    the current token never goes backward), so saturation can only make a
    logically older item read _fresh_ — extra work that the ASS-7 backstop
    re-derives — never make a fresh item read older. Write this
    per-comparison-class argument into the SCOPE-18 entry; the witness run
    below is the machine-checked spot check, the polarity argument is the
    soundness claim.
  - **`G` derivation.** Measure, do not guess: a TLC falsification ladder
    over `stepCompletion` at the reduced instance
    (`--invariant generations-≤-G0`; a violation reveals a reachable higher
    generation; raise `G0` until no violation) gives the maximum reachable
    generation per path; set `G = measured + 1` and record the measurement
    here. The probe is A6 (§4.0).
- Witness: a `quint run` reaching a saturated comparison is pinned as a
  regression run; the aliasing VC is discharged by the inductive step check
  itself (§4). Raise rule: if the step check returns a saturation-aliasing
  counterexample, raise `G` by exactly one and re-run. If it still aliases,
  STOP and report (do not raise further, do not change saturation
  semantics).

### 2.4 Test points for §2 (binary, per slice)

After each table commit, in order:

1. `quint typecheck utils/quint/inductive/inductive-core.qnt`
   → PASS (exit 0 **and** no `QNT` error in output).
2. `quint test utils/quint/inductive/inductive-core.qnt`
   → same pass count as the recorded baseline (record the count on the
   unedited copy first; expected 70/70; binary: counts equal, zero failures).
3. Stop conditions: typecheck fails → fix the slice, do not proceed;
   suite count differs → the rewrite changed semantics, revert the slice.

Dupe-index decision gate: after making `dupeIndex` total with `Set[int]`
values, run the minimal inductive TypeOK check (§4, step 1). If Apalache
rejects set-valued maps, apply the boolean-matrix fallback, then re-run the
two test points above. One fallback only. The fallback rewrites every dupe
read site a second time (`dupeIndex.get(h).contains(id)` → matrix lookup);
it is a full second slice, not a small patch.

## 3. Invariant assembly

New module `inductive_proof` in `utils/quint/inductive/inductive-proof.qnt`, importing
`inductive_core` at the reduced instance (same entry pattern as
`utils/quint/proof/proof.qnt`, one import site = one instance). Define:

```quint
val I: bool = Domains and KeyCoherent and ReachInv and GuardSupport and PhaseCoherent
```

where each group is a conjunction of named vals so a counterexample names
its group.

Define `initI` alongside `I`: a nondet init that draws every state component
within the §2.1 finite domains and constrains the draw with `I` (one import
site; the draw space is the product of those domains). It is the init for the
§4.4/§5 state-property checks.

### 3.1 State and the phase machine's role

`I` ranges over all 16 vars (§2.1). `stepCompletion` fires environment
actions in phase 0, drain/work actions in phase 1 (one budgeted fault back
to phase 0), and tracked scans in phase 2 — so `I` must be preserved by
**environment actions too** (they are in the step). Consequence for the
iteration: any round whose counterexample is a phase-0 environment action
means a conjunct is not environment-stable → restate it in
environment-stable form or add the supporting conjunct; never narrow the
environment action.

### 3.2 Initial conjunct list and order of addition

Start with exactly this list (order matters; add top-down, one per round):

1. `Domains` — every conjunct in the right column of §2.1 (totality, ranges,
   subset bounds, `phase`/`faults` ranges), all in `setOfMaps`/set-domain
   form per the §2.1 Apalache-form note. Nothing else.
2. `WithinBounds`, `AssetIdKeyCoherent` (ported to sentinel form),
   `IdBudget` — present records (`kind != "absent"`) at most
   `PATHS.size() + 1` (album + one per path); this is what guarantees
   `nextFreeId` finds a slot whenever allocation fires (§2.1).
3. `DupeCompleteRow`, `MetadataCompleteRow`, `PathConsistentRow`,
   `ConvergedFlagSetRow` per path (ported verbatim from `proof.qnt`
   `:156-177`, presence tests in sentinel form) — the shape track's banked
   reachability core.
   3b. Album-row after `DbLoss` — resolved in Phase 0 (A3, §4.0), not here.
   Inspection to date: only `init` creates the album row
   (`initialAlbumAssetRecord`/`initialAlbumMetadataRecord`); no action
   re-creates id 0 after `DbLoss` wipes it, and the album-delta writes
   (`TrashToggle`, `AlbumShareEdit`, the member deltas) read
   `metadataPayload.get(0)` unguarded. `Converged` is per-path and does not
   read id 0, so the goal may survive — but the unguarded reads are
   ill-defined in `merged.qnt` on wiped maps and sentinel-total in the
   core. A3 disposes of this (fidelity fix in `merged.qnt` vs recorded
   encoding delta) before any slice is cut; the outcome seeds either the
   fix (core re-copied) or a conjunct/encoding note here.
4. Guard-support conjuncts from the five CEGAR rounds: resolvability
   (worker/scan reads resolve), lease-held (leased endpoints have rows and
   records), identity-freshness (skip-unchanged identity match),
   eager-evaluation totality (every `.get` guarded), batch accumulation
   (sweep submissions accumulate, never replace).
5. `SweepRecheckBatchClean`-style batch/generation coherence and
   `NoLostUpload`/`NoPermanentStale`-shaped intake coherence (state forms
   only; no temporal content).
6. `PhaseCoherent` — `phase == 0 or phase == 1 or phase == 2`;
   `scannedRoots` never shrinks (it only `.union`s), while `phase` may
   regress to 0 on the budgeted fault — so the watermark is monotone, not
   the phase; `faults <= 1`;
   `BeginScan` needs `Quiescent` (already in the action; the conjunct records
   the phase-1 exit shape).

Each addition is one round (§4). Do not add two groups in one round.

This list is a starting set, not a guarantee: it is coherence invariants.
The final `I` will also need goal-directed, phase-progress-aware conjuncts
discovered by the round loop — conjuncts of the form "at the phase-2 scan
watermark, per-path agreement holds", broken down per divergence shape. That
is what the heal check in the shape track established graph-exhaustively; it
must now be restated as a global invariant. Expect later rounds to add
these; do not treat the seed list closing as done.

## 4. Check protocol (Apalache inductive + TLC alternative)

### 4.0 Pre-flight (once, before Phase 1)

Control check:

```sh
quint verify --help | grep -i inductive
```

Expect a `--inductive-invariant` flag line. If absent: STOP, report the
installed Quint version (`quint --version`) and do not invent a flag
spelling.

Pre-flight items, each with its outcome recorded in this plan:

- **A1 — baseline pin.** Record `quint test
utils/quint/inductive/inductive-core.qnt` on the unedited byte copy at
  the core's own (canonical) bounds; expected 70/70. This count is the
  per-slice regression gate (§6). Proof entries at reduced bounds are
  verify vehicles, not the regression gate. If any run asserts whole-map
  structure, give it a sentinel-form mirror in the core during the owning
  slice.
- **A2 — Apalache harness.** Start one persistent Apalache server
  (`--server-endpoint`; the runner's pass-through is already supported) for
  all verifier runs of Phases 3–4, and batch verification conditions per
  request instead of one request per conjunct. Calibrate with the TypeOK
  run (§4.1): the spike measured 42 VCs ≈ 190 s cold; with a warm server
  the per-round cost is the step check only, which gives the 30-minute
  stop real headroom.
- **A3 — album-row disposition (blocks Phase 1).** Resolve §3.2 item 3b by
  inspection and, where needed, a probe run: is the album row (id 0)
  re-created by any action after `DbLoss`, and are the album-delta reads
  (`metadataPayload.get(0)`) guarded? Outcomes: (a) a `merged.qnt`
  fidelity fix is needed (album reads gain presence guards mirroring the
  code's album-row rebuild) — the fix lands on the model first as its own
  commit, `inductive-core.qnt` is re-copied, and the fix never touches the
  untouchable predicates; or (b) the reads are unreachable in
  violation-relevant states and the total-map encoding makes them total —
  record the behavioral delta (`merged`: undefined read vs core: sentinel
  record) as an encoding note in §3.2 item 3b. If neither disposition
  holds: STOP and report; do not patch the model inside this plan.
- **A4 — implication cost probe.** Price §5 before building `I`: with
  `initI`-equivalent (draw constrained by `TypeOK`) and
  `--invariant TypeOK` (trivially true), run the implication form once and
  record the cost. Over budget → the §5 stop rule will bite; batch or
  split the implication per conjunct group before Phase 3, not after.
- **A5 — assumption reconciliation.** Bring the §1 assumption list and the
  registry statuses (`assumptions.json`) to agreement: update statuses
  where gate evidence exists, otherwise keep `open`/`unverified` and quote
  them verbatim in the claim (§6).
- **A6 — `G` derivation probe.** TLC falsification ladder for the maximum
  reachable generation per path at the reduced instance (feeds §2.3).

Environment for every verifier run (same as the shape lane):

```sh
export JRE_BIN="$HOME/.quint/jre/bin"; export PATH="$JRE_BIN:$PATH"
export TMPDIR=/home/codesam/tmp/opencode/proof/tmp; mkdir -p "$TMPDIR"
export JAVA_TOOL_OPTIONS="-Djava.io.tmpdir=$TMPDIR"
```

Long runs redirect to `/tmp/opencode/logs/<date>-inductive-<round>.log`;
inspect via `tail`/`grep`. Classify by output text, never exit status.

Apalache hygiene (from the spike's measurements): Snowcat typechecking costs
~200 s per request regardless of content. Run Apalache in server mode
(`--server-endpoint`, pass-through is already supported by the runner) and
batch verification conditions instead of one request per conjunct. Per-round
stop rule: 30 minutes wall — if a round exceeds it, stop, batch or split the
invariant, and record the partial result. Never just wait.

### 4.1 Minimal inductive TypeOK check (first, before any invariant work)

```sh
quint typecheck utils/quint/inductive/inductive-proof.qnt
quint verify utils/quint/inductive/inductive-proof.qnt --main inductive_proof \
  --init initCompletion --step stepCompletion \
  --invariant TypeOK --backend apalache --inductive-invariant TypeOK \
  > /tmp/opencode/logs/<date>-inductive-typeok.log 2>&1
```

(`TypeOK` is the `Domains`-only conjunction from §3.2 item 1.) Expected:
base-case PASS with a VC count line (spike reference: 42 VCs, ~190 s);
output must name both initiation and consecution with no counterexample. If
Apalache emits map-domain errors: the redesign is incomplete — STOP, record
the exact variable and error, return to §2. If the flag form differs, use
the exact form from §4.0 and record it here.

### 4.2 Round protocol (the iteration loop)

A **round** is: (a) select the next conjunct from §3.2 (or the violated one
from the previous round), (b) add/strengthen exactly one conjunct, (c)
**pre-filter** the candidate — falsify it cheaply first with TLC at the
reduced instance over `stepCompletion` (minutes); a conjunct that fails
there is wrong: fix it offline, never submit an untested candidate to the
Apalache loop, (d) run typecheck + the §4.1 command with `TypeOK` replaced
by `I`, (e) record: round number, conjunct changed, base verdict, step
verdict, states/VCs, runtime, and — on failure — the violating action and
conjunct.

Attribution: the VC output names the failing action; classify each
counterexample as environment, pipeline, or fault action. A phase-0
environment action means the conjunct is not environment-stable → restate
it in environment-stable form (§3.1). Diagnostic-only narrowing is
permitted as proof debugging: run consecution once with environment actions
excluded to localize a failure — but the quoted verdict is always on the
full `stepCompletion`; a diagnostic run is never quoted as a result.

```sh
quint verify utils/quint/inductive/inductive-proof.qnt --main inductive_proof \
  --init initCompletion --step stepCompletion \
  --invariant I --backend apalache --inductive-invariant I \
  > /tmp/opencode/logs/<date>-inductive-r<N>.log 2>&1
```

Iteration rule: counterexample names action A and conjunct C →
strengthen C (or add C's missing support from §3.2 items 4–5) → next round.
Forbidden moves: narrowing an action guard, capping a counter, removing a
`stepCompletion` branch, touching the goal predicates. Any of these is STOP

- report, not a fix.

### 4.3 Stopping criterion (no counterexample / all VCs discharged)

Done iff in one run: initiation PASS **and** consecution PASS (output names
both, VC counts > 0, no counterexample trace) **and** §5 implication PASS.
Otherwise continue rounds. Hard stop: 20 rounds without converging to a
smaller failure set (last 5 rounds each fail on a different conjunct with no
repeats fixed) → STOP, record the failing VC set, go to §8 fallbacks.

### 4.4 TLC alternative

TLC is the fallback when Apalache stalls on a round (not a replacement for
the inductive check — TLC cannot do induction). Do **not** point TLC at
`initI`: its draw space is the product of the §2.1 domains and TLC would
never enumerate it (see the §5 budget note). Point TLC at a _bounded drawn
family_ instead, as the shape track did:

```sh
quint verify utils/quint/inductive/inductive-proof.qnt --main inductive_proof \
  --init <family init or a pinned witness state> --step Skip \
  --invariant CompletionInvariant --backend tlc \
  > /tmp/opencode/logs/<date>-inductive-tlc-impl.log 2>&1
```

(`Skip` stutter makes this a finite state-property check over that family's
states at reduced bounds.) Expected: `[ok] / No violation found`, queue 0,
complete depth. Use only for anti-vacuity spot checks and per-family
soundness, never as the inductiveness argument or the §5 implication.

### 4.5 Core↔merged differential (standing gate, run after Phase 2)

Paired forward-simulation over a fixed seed list (recorded; start with 20
seeds × 100 samples): run `merged.qnt` (reduced entry) and
`inductive-core.qnt` under identical seeds and sample counts, and compare
per step (1) the action-name sequence (ITF `mbt::actionTaken`) and (2) the
goal-relevant projection — per-path row/record/metadata presence and
content, intake size, `Converged`, `AspectsAligned` — comparing projected
fields wherever the encodings align. Harness under
`utils/quint/inductive/tools/` (committed, derived). Any divergence: STOP,
classify as rewrite bug (fix the codemod, regenerate, re-run the owning
slice's test points) or a genuine model delta (file it, re-copy). This gate
re-runs after any later change to either model. It is the semantic half of
the core↔merged bridge; the 70-run suite is the assertion half.

## 5. Goal implication

Prove `I => CompletionInvariant` as a state-property check (no temporal
content: every `I`-state at phase-2 quiescence satisfies the goal):

```sh
quint verify utils/quint/inductive/inductive-proof.qnt --main inductive_proof \
  --init initI --step Skip --invariant CompletionInvariant --backend apalache \
  > /tmp/opencode/logs/<date>-inductive-impl.log 2>&1
```

Expected result: PASS / no violation (Apalache), or per §4.4 the TLC form
with `[ok] No violation found`, queue drained, complete depth. Binary:
PASS → the theorem's clause 3 holds; violation → the counterexample is an
`I`-state violating the goal, so `I` is too weak: add the missing conjunct
(one round, §4.2) and re-run §4.2 + §5. A passing implication with a failing
inductiveness (§4.2) proves nothing alone — quote only all three together.
Budget note: `initI` draws the product of the §2.1 domains — astronomically
large for TLC. Use Apalache here (symbolic); do not run TLC on this check.
The cost is priced in A4 (§4.0) before Phase 3; if over budget, batch or
split the implication per conjunct group before entering rounds. Stop rule:
30 minutes wall → go to F2.

## 6. Test points per step (binary summary)

| Step                         | Command                                                                                    | Pass                                                | Stop                                                                                                                           |
| ---------------------------- | ------------------------------------------------------------------------------------------ | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| 0 control                    | §4.1 command against the **unmodified** copy                                               | fails with map-domain errors (reproduces the spike) | passes → representation already encodable; skip to §3                                                                          |
| 0 pre-flight                 | A1–A6 (§4.0)                                                                               | all recorded; A3 disposition decided                | A3 unresolvable → STOP; A4 over budget → batch/split §5 before Phase 3                                                         |
| 1 slice _k_ (7 slices, §2.2) | `quint typecheck .../inductive-core.qnt`                                                   | no `QNT` error                                      | any error → fix slice, do not proceed                                                                                          |
| 1 slice _k_                  | `quint test .../inductive-core.qnt`                                                        | count equals recorded baseline (expect 70/70)       | count differs → revert slice                                                                                                   |
| 1 dupe decision              | §4.1 command                                                                               | Apalache accepts the encoding                       | rejects sets → boolean-matrix fallback, re-run slice tests once                                                                |
| 2 epochs                     | `grep -n "generations.get(.*) + 1" inductive-core.qnt` empty; typecheck; suite count equal | all three hold                                      | any fails → fix, do not proceed                                                                                                |
| 2 witness                    | pinned saturated-comparison `quint run`                                                    | run passes, comparison observed                     | unobservable → STOP, saturation semantics unsound                                                                              |
| 2→3 differential             | §4.5 paired runs                                                                           | no divergence                                       | divergence → fix codemod or file model delta, re-copy, re-run slice tests                                                      |
| 3 round N                    | §4.2 command                                                                               | base PASS + step PASS                               | new failing VC → next round per iteration rule                                                                                 |
| 3 hard stop                  | round count                                                                                | ≤ 20 rounds and failure set shrinking               | else STOP → §8                                                                                                                 |
| 4 implication                | §5 command                                                                                 | PASS / no violation                                 | violation → one §4.2 round, re-run §4.2 + §5                                                                                   |
| final                        | all of §4.2 (final `I`) + §5                                                               | both PASS in the same revision                      | quote only the triple (§1 clauses 1–3) with instance, bounds, and assumption statuses verbatim (A5) plus the result chain (§1) |

Regression gate throughout: the 70-run suite on `inductive-core.qnt` stays green
after every edit; `just quint check` / `just quint test` on `pipeline/`
are untouched and must stay green (they do not cover the new directory).
After Phase 2, the §4.5 differential joins the gate: it re-runs after every
later edit to either model.

## 7. File layout and gates

New directory `utils/quint/inductive/` (mirrors the `utils/quint/local/`
precedent: keeps the closed-world `specs`/`step` gates, SCOPE catalog,
traceability, and docs run counts pinned to `pipeline/`):

- `utils/quint/inductive/inductive-core.qnt` — module `inductive_core`: the
  total-map + epoch-token redesign (§2). Starts as a byte copy of
  `pipeline/merged.qnt` (including its runs, so `quint test` is the
  regression gate), then the 7 slice commits. Generated or mirror-checked
  per the §2.2 drift rule — it is a derived artifact of `merged.qnt`, never
  an independent model.
- `utils/quint/inductive/inductive-proof.qnt` — module `inductive_proof`: imports
  `inductive_core` at the reduced instance (one import site), defines
  `TypeOK`/`Domains`, `I`, `initI`, and the implication plumbing (§3–§5).
- `utils/quint/inductive/entries/` — reserved for the canonical-bounds
  import if the scale rung is attempted; empty until then.
- `utils/quint/inductive/tools/` — the per-table codemod scripts (§2.2
  drift rule) and the §4.5 differential harness. Committed; the core file
  is their output.

Justfile recipe (advisory, nightly — explicitly **not** a merge gate and
changing no existing gate):

```just
# utils/quint/justfile addition (new recipe only; check/test/lint untouched)
[group('quint')]
inductive-check: _install-quint
    quint typecheck "{{ source_directory() }}/inductive/inductive-proof.qnt"
```

State in this plan and in the recipe comment: until G1 lands, the
inductive lane is advisory; `just quint check`, `just quint test`, and
the 70-run suite remain the merge-gate regression. Adding later Apalache
invocations to the recipe needs no gate change provided they stay inside
this recipe.

## 8. Fallbacks and decision gates

Enter only on the stated stop condition; each fallback has its own effort
cap and stop rule.

- **F1 — drawn-family + TLC resource tuning.** If the redesign proves
  infeasible at a specific slice (two failed attempts at the slice's test
  points), return to the shape track: split the cap-bound full
  cross-product family, raise TLC memory/time on scratch only, per-dimension
  attribution as in `.plan/quint-proof-lane-longcat.md` Learnings
  2026-11-22. Effort cap: 3 days. Stop: no new family exhausts → F3.
- **F2 — inductive step stalls past the §4.3 hard stop.** The core↔merged
  refinement question is already answered at F2 entry by the standing
  differential (§4.5), so F2's remaining work is invariant-side: re-derive
  the stalled conjunct set from the shape-track artifacts (`proof.qnt`
  families and reachability invariants), retry with per-group VC batching,
  or accept the scoped shape-track claim as the lane's output. Effort cap:
  1 week. Stop: any irreconcilable semantic delta found late → F3. (The
  model↔code link stays out of scope regardless.)
- **F3 — close the lane.** Keep the shape-track evidence, the 70-run
  suite, and this record. The honest claim remains the scoped shape-track
  sentence plus sampled execution. No further new plan documents.

Decision gate order: §2 slice failure (twice) → F1; §4.3 hard stop → F2
then F3; §2.3 second saturation aliasing → F1 (epochs are the risk point).

## 9. Effort estimate and sequencing

| Phase | Work                                                                                   | Estimate  | Depends on                    | Parallelizable                                            |
| ----- | -------------------------------------------------------------------------------------- | --------- | ----------------------------- | --------------------------------------------------------- |
| 0     | Pre-flight: control spike, harness, baseline pin, A1–A6 (§4.0–§4.1 on unmodified copy) | 2–3 days  | —                             | A3/A4/A6 partly parallel                                  |
| 1     | Representation redesign, 7 slices via committed codemods (§2.1–§2.2, §2.4)             | 2–3 weeks | 0                             | slices independent across workers; integration sequential |
| 2     | Epoch tokens + comparison sites + polarity argument and G derivation (§2.3)            | 3–5 days  | 0 (joins 1 at the last slice) | no                                                        |
| 2→3   | Core↔merged differential gate (§4.5)                                                   | 0.5–1 day | 1, 2                          | —                                                         |
| 3     | Invariant rounds with TLC pre-filter (§3–§4, ≤ 20 rounds)                              | 1–2 weeks | 2→3                           | no (one conjunct per round)                               |
| 4     | Implication + quoting (§5–§6)                                                          | 0.5 day   | 3                             | —                                                         |

Total: 5–7 weeks for the reduced-instance triple. Canonical-bounds
re-run (same files, new constants) is a +2–3 day rung after Phase 4,
quoted separately. A less capable model executes strictly top-down:
finish each row's test points before the next row.
