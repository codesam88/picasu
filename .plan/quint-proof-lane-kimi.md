---
status: done
type: feature
priority: medium
area: testing
---

# Formal validation direction for the pipeline model — execution playbook

Audience: an engineer (human or AI) executing this plan **without deep Quint/model-checking
experience**. Every step is explicit: exact patches, exact commands, binary pass/fail
criteria. If a criterion is not met, stop and report — do not improvise.

Status: CLOSED. Steps 1–3 and step 4 landed; T1 and T2 executed; no
machine-checked bounded verdict. The full-machine bounded lane is closed;
local protocol proofs are the approved follow-on (§9).
Context: `.plan/quint-proof-lane.md` (the concluded exploration, measurements,
and method findings). Model: `utils/quint/pipeline/merged.qnt`
(`pipeline_merged`, ~3,300 lines).

## Execution outcome (2026-11-20)

- Step 1 anchor audit completed (id quotient safe with conditions; generation
  comparison inventory). Step 2 controlled A/B passed for the fault-free suffix:
  uncapped arm 600 s timeout at 4.3M distinct / 2.3M queued; with `genSum < 8`
  on the two phase-1 `ScanRoot` branches, 16,670 distinct, queue 0, complete
  depth 29, 36 s, `[ok] No violation found`; witnesses alive.
- Step 3 landed as `24cfb13c` (R=8, G=12, SCOPE-14), `1861b60b` (submit
  freshness, SCOPE-15), `bd600448` (`val freeId` opaque-id quotient, SCOPE-16).
  Lint, 70-run suite, `just quint check`, and pre-commit gates passed for each.
- Step 4 mutation checks were scratch-only: R=0 left the 70-run suite green;
  removing one freshness conjunct raised reduced TLC distinct states ~8%;
  removing one phase-0 cap lowered them ~8% in one 300 s sample. The unmutated
  reduced control timed out at 12,048,992 distinct / 11,710,729 queued / depth 6.
- Generator fix `9ae4c749` made `prefixes.ts gen` emit 46 classes / 47 elements.
  T1 (`initAllPinned`, 2 h TLC) timed out at 1,467,806,149 generated /
  402,641,750 distinct / 392,688,304 queued with no verdict marker; scratch hit
  74 GB and was cleaned. T2 (`prefixes.ts run`) produced 47/47 QNT409 compile
  errors because generated `initAllPinned` makes `initP<k>` appear on both the
  init and step sides. Zero model verdicts from T2.
- Closure reason: `CompletionInvariant` is a safety implication at quiescence,
  not a convergence/liveness claim; the caps change the checked system; small
  scope is a falsifier without a cutoff theorem. Details in
  `.plan/quint-proof-lane.md`, “Method findings”. No further full-machine TLC,
  T2 smoke, or scheduler reductions are authorized.

---

## 0. Rules that apply throughout

1. **Never weaken the property to get a verdict.** `Converged`, `AspectsAligned`,
   `CompletionInvariant` are untouchable. All changes below are _scheduler policy_
   (step-branch guards) or _representation quotients_ (how a value is encoded), never
   property edits.
2. **Policy lives in the step, fidelity lives in the actions.** The model's own
   division of labor (`.plan/quint-proof-lane.md`, "Step/action division of labor"):
   guards inside an `action` body describe what the code does; conjuncts added to
   `stepCompletion` branches describe which executions the checker visits. Every
   budget/cap below goes in a **step branch**, never in an action body.
3. **Measurement discipline** (this lane's norm): every claim is settled by a
   recorded run, not by argument. Log every verifier run: command, distinct states,
   depth reached, runtime, verdict. Long-running commands redirect stdout/stderr to
   `/tmp/opencode/logs/<date-time>-<cmd>.log` and are inspected via `tail`/`grep`
   (repo rule in AGENTS.md).
4. **Outcome classification is output-based, never exit-status** (the 12/12 artifact,
   plan file exploration 2). For this toolchain a pass requires the repo classifier's
   `no violation found` or `the outcome is: noerror`; older renderings
   `No error found` / `finished` are also recorded. Any later error marker wins. A
   Quint/Apalache exit 0 without a verdict marker is NOT a pass.
5. After **every** model edit: `quint typecheck utils/quint/pipeline/merged.qnt`,
   then `just quint test` (the 70-run suite) before any verifier run.
6. Work in small commits on the investigation branch so each patch is revertible on
   its own.

---

## 1. Why the previous attempts failed — findings

### F1 — The reachable state graph is infinite by construction (the keystone finding)

A model checker can only exhaust an **finite** graph. This model's graph is infinite
because several `int`-valued state components grow without bound under the step as
written. Complete inventory — every unbounded source, its bump site, its bound:

| #   | State component                                         | Bump site                                                                                                                                                                                                                                                                      | Guard today                                                                                                                                                                                                   | Bound mechanism                                                                                                                                                        |
| --- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `generations: str -> int`                               | `SubmitScan` (844), `SubmitWatcherIndex` (859), `SubmitWatcherRemove` (874), external events (452/512/531/549/668/689/741/1600/1624/1788/2045), `ScanRoot` (1060, phase 0 at 2933–2934 and phase 1 at 3122–3123), `MoveLeaseAcquire` (1730, +2 per fire, phase 0 at 2968–2987) | Several are repeatable forever on finite file values; phase-1 faults (3110–3121) reboot to phase 0, reopening all of them. Lines 3074–3093 are `MoveRowRewrite`, which stutters at 1773 — not a bump channel. | Probe: cap only the **two** phase-1 `ScanRoot` branches. Full kit: global generation-sum cap on every bump-contributing `stepCompletion` branch, plus submit freshness |
| 2   | `intake: Set[IntakeItem]`                               | the same submit/scan actions                                                                                                                                                                                                                                                   | items carry the current generation token, so each bump mints a _distinct_ item                                                                                                                                | Bounded transitively once (1) is bounded and phase-0 excursions are scoped: finitely many distinct items ⇒ finite powerset                                             |
| 3   | `nextAssetId: int`                                      | `WorkerApplySuccess` (930) and `UploadIndexed` (2087), both on a path with no row; `DbLoss` resets to 1 (646)                                                                                                                                                                  | unbounded across delete/re-add cycles                                                                                                                                                                         | Smallest-free-id quotient (Phase 1) — must patch **both** allocation sites                                                                                             |
| 4   | `metadataPayload` int fields (`itemCount`, `itemSize`)  | album deltas                                                                                                                                                                                                                                                                   | derived from                                                                                                                                                                                                  | PATHS                                                                                                                                                                  | and `SMALL` sums | Already finite given the const bounds |
| 5   | `files` fields (`assetHash`, `mtime`, `len`, `sidecar`) | environment events                                                                                                                                                                                                                                                             | drawn from `HASHES`/`SMALL`                                                                                                                                                                                   | Already finite given the const bounds                                                                                                                                  |

Consequences already observed in the exploration, re-read through F1:

- Every TLC run on `stepCompletion` was non-terminating **in principle**, at every
  bound configuration, for every prefix — including the empty one. The recorded
  signature ("234k at depths ≤6, then 6M→45M per depth, still growing") is
  accelerating frontier growth, which is what an unbounded counter produces; a finite
  graph's frontier eventually turns down.
- The exploration's decisive probe — "the empty prefix explodes, so the explosion is
  the drain+heal interleaving product independent of inputs" — ran `stepCompletion`,
  which admits the unbounded phase-1 rescan loop **and** fault branches that reopen
  the unbounded phase-0 submit/lease loops. The probe was confounded twice: it measured
  counter+queue growth from both loops as well as interleavings. The rescan budget R
  was demoted on the strength of that probe; the controlled A/B in §3 closes the first
  confound, and §4.2/§4.4 must close the second before a full claim.
- F1 is **code-certain** (follows from the guards as written) but **impact-unmeasured**
  (how much of the 41M it accounts for is empirical). The Phase-0 probe settles it
  before any larger commitment.

### F2 — `generations` is ordering machinery whose values never appear in the property

`Converged` / `CompletionInvariant` reference `files`, the aspect tables, and emptiness
of the in-flight structures. Generation _values_ appear nowhere in the goal. The full
comparison inventory (audit, step 1) has 11 sites:

| Use                         | Sites (merged.qnt)                                                                      | Comparison                                  |
| --------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------- |
| Worker applicability        | 901, 960, 981, 1016, 1037                                                               | `it.generation == generations.get(it.path)` |
| Supersede                   | 942                                                                                     | `it.generation < generations.get(it.path)`  |
| Upload/pickup applicability | 2073, 2105                                                                              | `it.generation == generations.get(p)`       |
| Changed-since-enumeration   | 1670                                                                                    | `sweepBatch.get(p) == generations.get(p)`   |
| Settled-state predicates    | 286–287 (`NoLostUpload`), 2190 (`SweepRecheckBatchClean`, defined but never referenced) | equality / `>`                              |

An unbounded `int` map for these finite comparisons is overkill. It is also the item
the inductive spike flagged as "the creative item" blocking Apalache's map-domain
encoding (plan file, exploration 5). Bounding it (F1) serves TLC; quotienting it to a
finite token type (Phase 3) serves Apalache.

### F3 — `nextAssetId` values never appear in the property either

`Converged` checks key existence and field equality, not id values ("assetId values
(move preservation is a move property, not a convergence property)", merged.qnt:324ff).
The code allocates ids by random hash; the model's ever-increasing counter is one
possible allocation. Smallest-free-id allocation is an _opaque-id quotient_ and is
finite by construction. Step-1 audit verdict: **SAFE-WITH-CONDITIONS**, not an
unqualified fact:

- Only `AssetIdKeyCoherent` (2720–2721) reads `.assetId` values; it holds by
  construction (key == value at allocation; moves/init preserve it).
- Five run assertions read concrete ids (1220, 1270, 1402, 1415, 1515). All are
  distinctness or move-preservation assertions; none should break, but the full suite
  is the canary and must be re-run, not edited.
- Occupancy argument: `assetById` holds ≤ |PATHS| media rows plus the album row (id 0),
  so `1..|PATHS|` always contains a free id; the `PATHS.size()+1` fallback is
  unreachable. Torn states only reduce occupancy.
- Patch must cover **both** allocation sites (930, 2087); `nextAssetId` assignments
  elsewhere (including `DbLoss`'s reset to 1) stay as stutter/constant for now.

### F4 — The step enumerates ~370 per-argument branches (secondary)

`step` / `stepCompletion` list every action × argument tuple literally (required by the
branch-completeness gate). For TLC this is equivalent to `nondet` enumeration; for
Apalache it inflates the transition relation — the plausible driver of the measured
90/107/610 s per-depth cost. Mechanical to fix for a verify target (Technique T5).

---

## 2. The strategy in one paragraph

Make the graph finite by bounding **work generation** (submits, rescans, id
allocation) while leaving **work consumption** (the drain+heal machinery, which is
what the property is about) fully general — then re-run the exact probes that failed.
The budgets are scoped claim parameters quoted alongside the existing K=1 fault budget
(SCOPE-13), and each is validated for over-pruning by an explicit anti-vacuity
protocol (§6). Decision path:

```
Phase 0 probe (hours): A/B the phase-1 ScanRoot cap on a fault-free suffix
   ├─ uncapped grows, capped terminates → F1 confirmed for the suffix → Phase 1
   ├─ both terminate → growth was interleaving/fault-driven; stop for review
   ├─ both grow identically → F1 falsified for this channel → review before fallback
   └─ capped collapses but does not terminate → report numbers, stop for review
Phase 1 (days): land the finiteness kit (rescan budget + global generation cap for
   phase-0 fault excursions, submit freshness, id quotient, re-validation), then
   re-validate the 70-run suite + anti-vacuity witnesses + mutation checks
Phase 2 (hours–days): exhaustive TLC verdict on the full reduced instance, K=1
   ├─ verdict obtained → scale ladder (4 paths → canonical 5), each rung measured
   └─ too large even finite → Technique T4 (property decomposition by divergence
        shape, reusing the existing prefix harness)
Phase 3 (decision): claim sufficient? → done. Unbounded claim needed? → inductive
   track (finite-token redesign + Apalache --inductive-invariant; the spike's
   blockers are gone by then).
```

---

## 3. Phase 0 — controlled probe (PASS, isolated fault-free suffix)

Goal: isolate the phase-1 `ScanRoot` generation channel. Pass requires an A/B result:
the fault-free suffix grows without the cap and terminates with it. A terminating
scratch run alone does not validate the full machine.

Result 2026-11-20: arm A (uncapped) timed out at 600s with 4,316,486 distinct states,
2,314,698 still queued, depth 20 still climbing, no verdict. Arm B (two `genSum < 8`
conjuncts on ScanRoot r1/r2) completed in 36s with 16,670 distinct states, queue 0,
complete graph depth 29, and `[ok] No violation found`. Steps gate 1493/54 green;
rescan witness 130/200; antecedent witness 40/200. This confirms the channel for the
fault-free suffix only.

### 3.1 Corrections established during step 2

The step-1 audit's `MoveLeaseAcquire` claim was wrong. Verified against the tree:

- Lines 3074–3093 are 20 `MoveRowRewrite` branches; that action stutters
  `generations` at 1773. There are **no** phase-1 `MoveLeaseAcquire` branches; its
  branches are phase 0 only (2968–2987). Capping those 20 branches would be wrong:
  they are work-consuming drain machinery and an over-pruning risk.
- The only phase-1 generation-bump branches are the two `ScanRoot` branches
  (3122–3123). Phase-2 `CompleteScan` also bumps, but `scannedRoots` permits at most
  one scan per root.
- Fault and torn-window branches at 3110–3121 set `phase' = 0`. From phase 0 the
  submit/lease/scan loops are unbounded again, so a probe using plain
  `stepCompletion` from `initDrain` does **not** isolate the phase-1 channel.
- Repo verifier flags are `--init … --step stepCompletion --invariant … --backend tlc
--max-steps 60` (from `coverage/bin/prefixes.ts`); there is no `--next`. Success
  markers are lowercase `no violation found` / `the outcome is: noerror`, with the
  repo classifier giving any later error marker precedence. `quint typecheck` exits 0
  even on failure; grade by output text. TLC needs `~/.quint/jre/bin` on `PATH`.

First step-2 measurement: a scratch model with the two correct caps, run from
`initDrain` with plain `stepCompletion`, reached 21.1M distinct states at depth 7
(~4.9M/min) and died on `/tmp` disk quota with no verdict. Growth was statistically
unchanged from the recorded baseline. This is **not** a falsification: the fault
excursion remained enabled, so the probe was still confounded.

### 3.2 Patch (2 branch conjuncts + 1 val)

Add a **non-pure `val`** near the completion-machine section (e.g. above
`action Skip`):

```quint
  // Scheduler-policy cap (probe): bounds generation bumps in the phase-1 drain.
  val genSum: int = generations.keys().fold(0, (a, p) => a + generations.get(p))
```

Use it bare (`genSum < 8`), not `genSum()`: `pure def genSum(): int` with a call
site fails typechecking (`QNT000 Couldn't unify int and oper` + effect error), and
`pure val` fails with `QNT200`. Add the conjunct only to branches 3122–3123:

```quint
    all { phase == 1, genSum < 8, Stay(ScanRoot("r1")) },
    all { phase == 1, genSum < 8, Stay(ScanRoot("r2")) },
```

Why 8: `init` sets all generations to 0; `ScanRoot("r1")` adds ≤ 2 at reduced bounds,
`ScanRoot("r2")` ≤ 1. CAP=5 was computed before the lease-channel error was found;
it still covers two r1 rescans and one r2 rescan, but 8 leaves slack. Phase-2
`CompleteScan` branches are uncapped and need no reserve. If bounds change, recompute.
Track another SCOPE entry if the cap is kept as policy (§4.1).

This patch touches no action body and no branch list; the `@step-universe` gate stays
green. It does not bound phase-0 work generation; §4.2 is still required for any full
claim.

### 3.3 Entry: fault-free suffix via exhausted fault budget

Quint 0.33.1 does not accept a primed variable in a comparison expression, so
`not(phase' == 0)` is a parse error, and `not(phase == 0)` is vacuous from phase 1.
Exclude the fault excursion by exhausting the scheduler budget at init instead:

```quint
  action initDrain = all { init, phase' = 1, scannedRoots' = Set(), faults' = 1 }
  action stepNoFault = stepCompletion
```

All eight phase-1→0 branches (`CrashReboot`, `ThumbLossReboot`, `DbLossReboot`, and
five literal `RemoveIdOnly` branches) are gated on `faults < 1`; `Stay` stutters the
budget, so `faults = 1` disables exactly that family. `Quiescent` and
`CompletionInvariant` do not read `faults`, and the only `faults == 0` uses are run
assertions. `BeginScan` (1→2) and `CompleteScan`/`Skip` (phase 2) remain reachable.
This is a synthetic scheduler state for the isolated probe, not a state claimed
reachable from `initCompletion`; the full claim still has to include the fault path.
Do **not** filter with `phase' == 1`: it is both unparseable and would remove the
completion runs the invariant depends on.

### 3.4 A/B run

Variant A: unpatched tree model. Variant B: scratch copy with the §3.2 patch. Same
entry, same reduced bounds, same command, same wall-clock and disk budget:

```
quint verify utils/quint/pipeline/entry-probe-drain.qnt \
  --main entry_probe_drain --init initDrain --step stepNoFault \
  --invariant CompletionInvariant --backend tlc --max-steps 60
```

Run both in the background with stdout/stderr redirected to
`/tmp/opencode/logs/<date-time>-probe-{a,b}.log`; retain exit codes and command
lines. `--max-steps` is the repo's flag but TLC ignores it as a depth bound — the
external `timeout`/disk budget is the real cap. Put scratch state on a filesystem
with headroom (`/home/codesam/tmp/opencode/...`), set `TMPDIR` /
`-Djava.io.tmpdir` accordingly, and prepend `~/.quint/jre/bin` to `PATH`. Classify by
output, not exit status. Expected: A grows without a verdict; B terminates with a
success marker and `>0 distinct states`.

### 3.5 Criteria

- **PASS (isolated channel)**: A grows/no verdict; B terminates with a success marker,
  zero violations, and the §6 anti-vacuity checks green. Then proceed to §4, while
  recording that phase-0 excursions remain out of scope until §4.2 lands.
- **STOP AND REVIEW**: B collapses the frontier but does not terminate; or both
  terminate (the measured growth was then interleaving/fault-driven, not this
  counter). Record states/depth/rate for both arms.
- **DIRECTION QUESTIONABLE**: both grow at baseline rate under the fault-free filter.
  Do not claim F1 falsified from the confounded run; escalate before fallback.

If a terminating run reports a violation, do not patch the model in response.
Reproduce it with plain `quint run` first, per the lane's TDD pattern.

---

## 4. Phase 1 — land the finiteness kit (days)

Four changes, one commit each. After each: typecheck, `just quint test`, §6 checks.
Landed so far: `24cfb13c` (SCOPE-14 generation caps) and `1861b60b` (SCOPE-15 submit
freshness). Commit 3 (SCOPE-16 id quotient) is in progress; commit 4 is validation.

### 4.1 Rescan budget and global generation cap (keep the Phase-0 patch, tune the cap)

The probe cap becomes the landed rescan budget R=8 on the two phase-1 `ScanRoot`
branches. It is **not sufficient** for the full machine: after a phase-1 fault,
phase 0 admits repeatable external-event, submit, scan, lease, upload and sidecar
actions that also bump `generations`. Submit freshness (§4.2) removes duplicate
submits but does not bound those other channels. Land a second, explicit SCOPE entry:
`genSum < G` with G=12 on the **118 phase-0 branches** whose action bumps
`generations` — 20 `ExternalRenameFile`, 15 sidecar-external events, 5
`AppSidecarEdit`, 5 `RotateWriteSidecar`, 5 `ApiUpload`, 15 submits, 2 phase-0
`ScanRoot`, 1 `DirMoveFsOnly`, 20 `ApiMoveFsOnly`, 20 `MoveLeaseAcquire`, 5
`SidecarEditPartial`, 5 `UploadStoredOnly`. Keep phase-2 `CompleteScan` **uncapped**:
it is watermark-bounded to one fire per root and is the heal scan the invariant
needs. Never cap work-consumers such as `MoveRowRewrite` — they stutter
`generations` and an over-pruning conjunct would disable legitimate completions after
a phase-0 excursion. `DbLoss` resets the sum to 0 (new epoch). Record `why_safe` as
provisional, explicitly noting the composition with §4.2 and the outstanding T3
mover-check, and validate with the §6 witnesses.

If Phase 0 passed with slack, consider R encoded as the explicit budget var `rescans: int` for readability:
`initCompletion` sets `0`, `BeginDrain` resets it, the phase-1 `ScanRoot` branches
guard `rescans < 1` and increment, `Stay` stutters it. No phase-1 lease budget is
needed: `MoveLeaseAcquire` has no phase-1 branches. Phase-0 submissions are reduced
by §4.2, and phase-0 leases are bounded by the global generation cap above. QNT202
note: because nearly all branches wrap their action in `Stay(...)`, adding a var
requires assignments only in `Stay`, `initCompletion`, `BeginDrain`, `BeginScan`,
`CompleteScan`, `Skip`, the three `CrashReboot` actions, and the five literal
`RemoveIdOnly` branches — about 10 sites, all mechanical. The generation-sum cap
needs **no** new var and is the fallback if the var proves fiddly. Either way: record
the budgets in `pipeline/scope.json` as new SCOPE entries with `why_safe` marked
**provisional** ("a rescan with no intervening FS change observes the drained state
the previous rescan observed" is a claim to be checked by T3, not an established
lemma), and run `just quint check` (which runs `check.ts all`, including the catalog,
specs, steps and `doc-check` gates; there is no `just catalog-check` recipe). A new
SCOPE id needs three coordinated edits in the same commit: the id enum in
`catalog.schema.json`, the entry in `scope.json`, and at least one `// @scope SCOPE-N`
carrier in `merged.qnt` (new definitions such as `val genSum` / `val freeId` need
their own tag). Then run `just quint docs` to regenerate
`docs/eventual-consistency.md` and `node utils/quint/coverage/bin/check.ts specs
utils/quint/pipeline --generate` to regenerate `traceability.json`; commit both.

### 4.2 Submit freshness (phase-0 work generation)

Add to the `stepCompletion` phase-0 branches of `SubmitScan(p)`, `SubmitWatcherIndex(p)`,
`SubmitWatcherRemove(p)` (and to the same branches in plain `step` only if you intend
sampled runs to reflect the policy — default: leave `step` alone):

```quint
not(intake.exists(i => i.path == p and i.generation == generations.get(p)))
```

Rationale (record in scope.json, provisional): a duplicate submit for a path whose
current snapshot is already queued adds no divergence shape the first submit did not
add. **Warning**: this guard references `generations`, so it must sit in the branch
conjuncts, not the action body (rule 2). Expect `WorkerSkipSuperseded` coverage to
remain — one in-flight supersede chain per path still forms after any real FS change.

### 4.3 Opaque-id quotient (`nextAssetId`)

Add:

```quint
  // Smallest-free-id allocation: ids are opaque; properties never read values.
  val freeId: int =
    1.to(PATHS.size() + 1).filter(i => not(assetById.keys().contains(i)))
      .fold(PATHS.size() + 1, (a, i) => if (i < a) i else a)
```

Quint 0.33.1 rejects `freeId()` for a state-reading nullary `pure def` (`QNT000`:
`int` vs `() => _t`), so use a non-pure `val` referenced bare, matching `genSum`
and `rows`. Then `grep -n nextAssetId utils/quint/pipeline/merged.qnt`: replace
**every** allocation read (`else { nextAssetId }`) in `WorkerApplySuccess` (5 reads)
and `UploadIndexed` (5 reads) with bare `freeId`. Change only the two increment writes (`nextAssetId' =
... + 1`) and the `DbLoss` reset to `nextAssetId' = nextAssetId`; keep the variable
constant so the ~40 other stutter assignments and all `run` assertions stay untouched.
Never allocate id 0 (the album row). At most |PATHS| media rows + the album row
coexist, so `1..(|PATHS|+1)` always has a free id — that is the finiteness argument.
Do **not** edit any property to encode this: the argument is recorded as a provisional
SCOPE entry and checked by the existing concrete-id runs. Record in scope.json: refines
random-hash allocation; id reuse after free is a behavior the real allocator admits
with probability ~0 and no checked property distinguishes (F3). If any run asserts a
concrete id value, triage that run — do not silent-edit it.

### 4.4 Re-validate

After each of commits 1–3, in this order:

```sh
just quint lint   # prettier + tsc + typecheck; grade typecheck by output text
just quint test   # 70-run suite; ~6 min here, redirect to /tmp/opencode/logs
just quint check  # catalog + specs + steps + doc-check
```

Any new SCOPE id also requires `catalog.schema.json`, `scope.json`, an `@scope` tag
in `merged.qnt`, `just quint docs`, and `node utils/quint/coverage/bin/check.ts specs
utils/quint/pipeline --generate`, all in the same commit. The steps gate is unaffected
by branch-conjunct-only caps; verify with `just quint check` rather than assuming.
§6 anti-vacuity checks green.

- **Mutation check** (this lane's norm): (i) set the rescan cap to 0 → expect at
  least one completion run that needs a phase-1 rescan to fail (if none fails, the
  budget is dead code — report); (ii) remove the freshness conjunct on one submit →
  expect no suite failure (it is a scheduler reduction, not semantics) but a
  measurable state-count increase in the Phase-2 probe; (iii) remove the global cap
  from one phase-0 bump branch → expect the full reduced probe to stop terminating —
  record all outcomes.
  Revert each mutation immediately after observing.
- Append the Phase-0/1 measurement rows to `.plan/quint-proof-lane.md` (repository
  memory beats chat memory).

---

## 5. Phase 2 — the exhaustive reduced verdict (hours–days)

### 5.1 The run

Entry: `entry-reduced.qnt` as-is (3 paths / 2 hashes / 2 smalls), `initCompletion` +
`stepCompletion`, invariants `CompletionInvariant`, `NoLostUpload`, `NoPermanentStale`,
`AssetIdKeyCoherent`, `WithinBounds`. K=1 (existing fault budget), R from Phase 1.
TLC, no time cap for the first attempt; log to file. If it has not finished within
the recorded baseline's scale (say 2× the 44 GB pool / 273M states), stop and go to
Technique T4 rather than feeding it more resources.

### 5.2 Anti-vacuity (mandatory on a no-violation verdict)

All three, in order:

1. TLC's state count is > 0 and the run terminated (output markers).
2. `just quint test` green — in particular `completionScanTest` and
   `completionCrashScanHealsTest`, which deterministically reach phase-2 quiescence
   (the invariant's antecedent) under the landed budgets.
3. Sampled witness: `quint run` on the reduced entry with
   `--witnesses` for `phase == 2 and scannedRoots == ROOTS and Quiescent` — must be
   reached in > 0 traces.

### 5.3 The claim, worded honestly

"Exhaustive model checking at the reduced instance (3 paths / 2 hashes / 2 roots)
found no violation of `CompletionInvariant` over all event mixes and interleavings
with ≤ 1 fault and ≤ R rescans per root per drain attempt." Every number in that
sentence is a scope parameter already registered in `scope.json`.

### 5.4 Scale ladder

Each rung is its own measured decision: duplicate `entry-reduced.qnt` with 4 paths
(check `entry-reduced-paths.qnt` first — an intermediate entry may already exist),
then the canonical 5 (`entry-canonical.qnt`). Stop at the rung that exceeds an agreed
budget; the claim quotes the largest completed rung.

---

## 6. Anti-vacuity protocol (applies to every "no violation" result)

A verification run that never reaches the invariant's antecedent proves nothing.
After EVERY pass:

1. The verifier reported a positive verdict marker over > 0 states (output-based,
   rule 4).
2. `just quint test` is green — the deterministic completion runs reach phase-2
   quiescence under the landed budgets.
3. Sampled witnesses are alive: the antecedent witness (§5.2.3) and, for the probe
   specifically, `phase == 1 and genSum >= 1` (proves the capped rescan still
   fires — valid only for the probe's zero-sum init; do not reuse it on the full
   machine, where phase 0 can raise the sum before the drain).
4. Coverage: `coverage/bin/check.ts steps` green (no silently dropped branches).

If any check fails, the verdict is vacuous: fix the over-pruning before re-claiming.

---

## 7. Additional techniques catalogue

Ordered by expected value-per-effort. T1–T2 are recommended inside Phase 2; T3+ are
contingent or optional.

- **T1 — Disjunctive-init rerun (cheap intermediate verdict).** The exploration's
  disjunctive init (126 initial states covering the single-event partition) found the
  round-5 bug in 203 s but could never terminate. Under the finiteness kit it becomes
  a **terminating bounded check over the whole single-event partition** — a real,
  quotable intermediate claim before the full machine finishes. Reuse the existing
  generator (`coverage/bin/prefixes.ts gen`); do not hand-write elements.
- **T2 — Property decomposition by divergence shape (the partition design, rescued).**
  If the full reduced instance is finite-but-too-big, verify per event class: pin each
  class into the init (existing harness), exhaust the now-finite drain+heal suffix per
  element. The exploration abandoned this because "each element re-explores the suffix
  product"; with the suffix finite and small, per-element re-exploration is
  affordable. Keep the 24-vacuous-element lesson: an element whose guards need
  non-initial state is _unreachable_, not a failure — classify and move on.
- **T3 — Commutativity (mover) checks: upgrade the budgets from assumption to proof.**
  The rescan cap is honest as a scoped parameter, but you can machine-check that it
  loses nothing: prove `ScanRoot(r)` commutes with the drain actions it interleaves
  (firing A;B vs B;A from the same state reaches equal post-states for the relevant
  action pairs). Recipe: a scratch module with a snapshot var; run
  `initDrain.then(<A;B with snapshot>).then(assert(...))` vs the swapped order via
  `quint run`; equal snapshots ⇒ the interleaving order of that pair adds no state the
  other order lacks. Cost ~1 day; optional; the one technique here that requires real
  model-checking literacy — pair with review if attempted.
- **T4 — (this is T2's trigger, listed for numbering: see T2.)**
- **T5 — Verify-target step with folded `nondet` parameters (Apalache lane only).**
  Generate `stepVerify` mechanically: one branch per action with
  `nondet p = PATHS.oneOf()` instead of ~370 literal argument tuples. Shrinks the
  transition relation Apalache/Snowcat sees. Keep the canonical enumerated `step` for
  the run suite and the branch-completeness gate; the generated step is verify-only,
  like the prefix modules. Do not hand-maintain it — script it (the prefixes
  generator is the shape to copy).
- **T6 — Symmetry reduction (only if the scale ladder stalls).** Paths within a root
  are interchangeable; TLC supports symmetry sets, but Quint's TLC backend does not
  expose them — using them means hand-editing generated TLC config (fragile,
  drift-prone). Expected win ≈ |paths|! on symmetric instances. Only with a recorded
  note in the plan file; never silently.
- **T7 — Apalache hygiene for the inductive lane (from the exploration's own
  measurements).** Snowcat typechecking costs ~200 s per request regardless of
  content — batch checks; use server mode (`--server-endpoint`, pass-through already
  in the runner); classify outcomes by output markers (the committed selftest pins
  the classifier); bounded `--max-steps` works here (unlike TLC, which ignores it).
- **T8 — Violation handling (standing rule).** Any verifier violation is first
  reproduced with plain `quint run` (no special encoding), then fixed TDD-style with
  a mutation-checked regression run — the round-1..5 pattern. Never patch the model
  against a verifier trace alone.

---

## 8. Phase 3 — the inductive track (only if the unbounded claim is required)

Enter only after Phase 2 lands and only on explicit decision. The spike (plan file,
exploration 5) showed the invocation form works (TypeOK base case: 42 VCs, ~190 s)
and failed only on representation. The remaining work, in dependency order:

1. **Finite epoch tokens for generations.** Small ordered finite set (e.g. `0..G`)
   with saturating successor; re-express the three F2 comparisons over it. The one
   genuinely new proof obligation: **no aliasing** — a saturated token must never
   compare equal to a queued item that should read as older. The Phase-1 budgets make
   the maximum live-epoch spread concrete (bounded by submits+scans per attempt);
   pin it with a witness run that reaches a saturated comparison. If saturation
   semantics surprise you, stop — this is the step where subtle unsoundness lives.
2. **Total-map encodings** for the six sparse tables (`assetByPath`, `assetById`,
   `metadataPayload`, `dupeIndex`, `thumbPresent`, `sweepBatch`): total maps over the
   existing finite consts with sentinel values (or option types), mechanically
   rewriting reads behind `pure def` accessors. `intake` becomes a subset of a finite
   record powerset once item fields are finite (epochs from step 1, content from
   HASHES/SMALL).
3. **Apalache `--inductive-invariant`** against the banked conjunct skeleton from the
   five CEGAR rounds (resolvability, lease-held, identity-freshness,
   eager-evaluation totality, batch accumulation). Budget 1–2 weeks of invariant
   iteration; the re-derivability argument becomes a conjunct and removes the fault
   budget from the inductive claim.

Claim if completed: convergence with no budgets — the strongest statement on the
table. Decision input: whether Phase-2's bounded claim plus the 70-run suite already
covers the project's risk appetite. Often it does.

## 9. Approved follow-on — local protocol proofs

User decision 2026-11-20: after the bounded lane closed, do local protocol proofs.
Record further work in this file and `.plan/quint-proof-lane.md` only; no new plan
documents. No edits to `Converged`, `AspectsAligned`, or `CompletionInvariant`.

### Scope and order

1. **L1 sweep chain** (round-5 bug site): `SweepEnumerate`, `SweepRecheck`,
   `SweepRemoveBatchApply`; two roots, one pinned missing-file row per root.
   Invariant: absent-file rows pruned, present-file rows never pruned, and a
   second root's enumeration preserves the first root's batch. No environment
   events or generation bumps after init; finite by construction.
2. **L2 crash/heal residue**: the three base faults plus `ScanRoot`,
   `WorkerApplySuccess`, `WorkerSkipSuperseded`, `WorkerRemoveApply`,
   `IntakePickupFileGone`; one crash pinned in init. Check the residue
   characterization on the crash step and `NoLostUpload`/`NoPermanentStale`
   after heal. Inline the `ScanRoot` body instead of calling `CompleteScan` so
   `phase`/`faults`/`scannedRoots` stay out of the slice. A small per-path
   heal-submit budget is required and must be quoted in the claim.
3. **L3 intake ordering**: three submits, upload/worker actions, and
   `IntakePickupFileGone`; pinned divergences, no environment events after
   init, at most two submits per path. Check exactly-one-terminal-outcome per
   item and `NoLostUpload`. `WorkerSkipUnchanged` pulls the `rows` projection;
   decide during implementation whether the extra ~40 lines are worth keeping
   that outcome in the checked action set.

### Layout and drift control

Use `utils/quint/local/` with a shared action-free `local-common.qnt` (four
types, twelve vars, local `val` bounds, `ROOTS`, `rootOf`, `snapshotMatches`,
`freeId`, `rows`, `NoLostUpload`, `NoPermanentStale`) plus one file per slice.
A new directory keeps the closed-world `specs`/`step` gates, SCOPE catalog,
traceability, and docs run counts untouched; those recipes are pinned to
`utils/quint/pipeline/`. Keep copied action bodies byte-identical modulo
indentation with source-line citations in comments; no new SCOPE ids for local
bounds unless a gate actually requires them. Dependency-closure measurements:
25/38/36 declarations for L1/L2/L3 (~207/~373/~446 source lines); standalone
files ~250–380/~400/~490 lines after trimmed init and the L2 scan inlining.

### Verification and anti-vacuity

For each slice: `quint typecheck <file>`, then
`quint verify <file> --init initX --step stepX --invariant InvX --backend tlc`.
Classify by output markers, not exit status; require positive state count,
drained queue, and a complete-depth line. Add a named witness invariant in the
local module and check its negation for a counterexample trace (antecedent
reachable). Mutation canaries on scratch copies: L1 re-seed the pre-fix sweep
batch replacement and require a violation; L2 budget-0 must break the heal
witness; L3 delete one generation guard and require a violation. Stop a slice
at ~100k states or ~10 min — narrow it, do not raise budgets. Keep
`just quint check` and `just quint test` green unchanged; if a slice exposes a
real bug, reproduce with `quint run` and fix `merged.qnt` by TDD on the parent
track.

### Claims

Quote only per-slice facts with the exact instance, pinned init, and budgets:
e.g. “over all interleavings of the three sweep actions on two roots with one
deletion per root, TLC exhausts the reachable graph with no violation.” Never
compose L1/L2/L3 into `CompletionInvariant`, `Converged`, or a global
convergence claim; composition across sweep, worker, and crash remains an
explicit assumption.

## 10. Common failure modes (read before starting)

| Symptom                                               | Cause                                               | Action                                                    |
| ----------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------------- |
| QNT202 after adding a var                             | a branch misses the new assignment                  | add the stutter in `Stay` and the ~10 listed sites (§4.1) |
| Suite green, TLC "passes", but antecedent unreachable | cap too small / over-pruned guard                   | §6 protocol; raise cap; re-run                            |
| Verifier exit 0, no verdict text                      | preprocessing failure (known artifact)              | rule 4: not a pass; read the log                          |
| TLC memory blow-up at 44 GB pool                      | known from baseline; not a stop condition by itself | compare _depth and growth_, not just footprint            |
| A run asserting a concrete asset id breaks            | id quotient (§4.3)                                  | triage the assertion; never silent-edit runs              |
| Violation found                                       | possibly a real bug (this lane found one)           | T8: `quint run` repro → TDD fix → regression pin          |
| Snowcat dominates runtime                             | ~200 s/request typecheck (measured)                 | T7: batch, server mode                                    |

## 11. Risks and open questions

- **Reduction honesty.** Budgets narrow the checked execution class. The claim is
  quoted with R alongside K, both registered in `scope.json` as provisional until the
  per-guard no-new-divergence-shape argument is written down (T3 can machine-check
  the rescan one).
- **Epoch saturation (Phase 3.1)** is the one step that can introduce silent
  unsoundness; it has its own witness pin and an explicit stop condition.
- **F1's share of the measured explosion is unmeasured until Phase 0.** The probe is
  deliberately cheap and binary precisely so this direction dies in hours if wrong.
- **The open fidelity item** (album `itemCount` delta on torn re-index, carried in
  the plan file) is unaffected by this direction; fold it into the Phase-2 invariant
  set when it lands.
