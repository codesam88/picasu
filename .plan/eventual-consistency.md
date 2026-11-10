---
status: in-progress
type: feature
priority: high
area: backend
---

## Goal

Prove the index pipeline: every filesystem and API event converges
the redb projection (rows plus derived artifacts) to the filesystem,
reads of missing or in-flight assets degrade gracefully, and the UI
presents each consistency gap honestly instead of as authoritative
data.

Verdicts (S6/S7, 2026-11-10; catalog `specs/goals.json`):

- **G0 [COMMITTED] — safety floor**, narrowed to what is evidenced:
  the index removes or relocates an asset only in response to an
  explicit user action, and auxiliary/derived writes are atomic.
  Anchors: `thumbnail.rs:write_atomically`,
  `sidecar_edit.rs:write_bytes_atomically`,
  `xmp_write.rs:write_sidecar_content`. Torn-byte reads are an
  explicit non-goal of this model; the `API_READ_*` faults track
  serving behavior.
- **G1 [COMMITTED] — quiescent convergence**, anchored to
  `Converged` and the 33 runs (`specs/eventual-consistency-goal.qnt`).
  Conditional on quiescence plus a completed scan; the scan premise
  is owned by the periodic backstop below.
- **G2 [COMMITTED] — bounded convergence.** N = one backstop interval
  after quiescence: convergence by the end of the first backstop scan
  cycle that starts after changes stop (two scan cycles at most
  counting a scan already in flight). Anchored by
  `boundedConvergenceTest`; holds while the user enables the backstop
  (`QUINT-ASS-7`).
- **G3 [QUEUED] — read-after-write.** The upload path waits on the
  flush batch; assign and delete have no verified barrier. Needs an
  inbox-lag model or a per-route barrier gate.
- **G4 [DROPPED as a goal]** — the missing/in-flight serving
  contract is a product decision, not a provable property. Kept open
  in the backlog and in the `API_READ_*` faults.

**Fairness owner (decided):** two scan triggers. (1) A rescan on the
watcher's `Flag::Rescan` signal (inotify overflow, S3) repairs known
gaps; it exists only while the watcher is enabled. (2) A per-root
periodic backstop scan, which the user enables at regular intervals
(`QUINT-ASS-7`) and which supplies G2's bound. With the watcher off,
the backstop is the sole trigger. REQ-5 and REQ-6 track the two
triggers; REQ-7 tracks scan-vs-assign exclusion.

## Scope

The pipeline covers filesystem inputs (watcher plus scans) and API
mutations that run while indexing is in flight (assign-album and
directory moves, upload, delete). The redb index is a rebuildable
cache; the filesystem is the source of truth (`docs/design.md`).
Presentation states (skeleton, dimmed stale, tombstone, retry) are
part of the goal; visual design is not.

## Rules (settled)

These constrain the design; they are not assumptions about the world.

1. Watcher plus periodic rescan, never watcher alone. The rescan
   interval is the documented upper bound on convergence.
2. Debounce and order submissions; a change observed mid-pipeline
   supersedes queued state, never applies over it (generation counters;
   stale items are dropped, never applied).
3. Reads never observe partial writes (temp file plus rename, or
   immutable content-addressed paths). A torn read is corruption, not
   staleness.
4. Presentation distinguishes no-data from stale-data: skeleton only on
   first load, keep and mark old content on refetch, explicit error/retry
   and tombstone states.
5. Stale values must look stale where acting on them matters.

## Steps

- **S1 [DONE]: Map the events** — `specs/events.json` (23 events,
  all established: FS/API boundary events plus 4 agreed
  catastrophic events — `WATCHER_GAP`, `PROCESS_CRASH`,
  `DERIVED_STORE_LOSS`, `DB_LOSS`; no implementation-internal steps
  as entries), validated against `specs/catalog.schema.json`,
  `just events-check` gate,
  wired into `check`. The `SubtreeMove` model action and the four
  unmodeled catastrophic events are resolved in
  `specs/eventual-consistency.qnt` (see Decisions); 12 TODO
  code_refs remain (implementation not built). Each entry
  carries `description`, `effects` (settled state), and `faults`
  (error cases with status). Goals and assumptions converted to the
  same catalog style (`specs/goals.json`, `specs/assumptions.json`,
  `just catalog-check`): goals G0–G4 with anchors and decision
  fields, assumptions QUINT-ASS-1..7 with owner/status (model refs
  live in the generated map).
  `specs/assumptions.md` removed (registry moved to the JSON catalog,
  methodology rules moved into `specs/README.md`);
  `specs/TRACEABILITY.md` replaced by `specs/traceability.json`
  (written by `just spec-check --generate`, machine-readable only);
  `docs/eventual-consistency.md` is generated from the four JSON
  catalogs by `just docs-eventual-consistency`.
  Multi-file crash survey recorded:
  filesystem-first/DB-last ordering throughout, `rotate_image` the
  thumbnail-before-sidecar exception. Requirements agreed in-catalog
  and derived with gates at S10:
  atomic sidecar/albuminfo writes (implemented, gate pending);
  per-row thumbnail checksum (regenerate on mismatch); orphan
  `.xmp` logging on restart/scan (log-only, no action during
  in-flight mutations; log-only observation of an in-flight assign
  is expected); upload stale-tmp sweep
  (residue-free landing). Decisions adopted: crash-safe assign
  ordering (copy sidecar first, move, delete old); rotate ordering
  (sidecar -> thumbnail -> DB; sidecar is the rotation source of
  truth, present sidecar forces reprocess). Watcher-absent reads
  modeled (`ReadMissing`, `scanOnlyConvergenceTest` green).
- **S2 [DONE]: Establish overall goals** —
  The Goal and Rules above record the settled direction; goal
  candidates G0–GN were proposed there, and committing, queuing, or
  dropping each was S6. Related reading:
  `docs/briefs/inotify-filesystem-watcher.md`
  (watcher mechanics), `docs/briefs/formal-spec-enforcement.md`
  (enforcement pattern, ISA-worked example).
- **S3 [OPEN]: Honor the rescan flag** — first conclusion. An `Other`
  event with `Flag::Rescan` (inotify overflow, `notify-8.2.1`) must
  trigger a full rescan of the affected root instead of being ignored
  (`start_watcher.rs:273`). This is the mandatory half of the
  fairness owner, tracked as REQ-5; the model already expresses
  recovery as `ScanRoot`, so the work is code plus a forced-overflow
  test.
- **S4 [OPEN]: Recover from watcher errors** — first conclusion. The
  watcher `Err` arm only logs (`error.rs:254`). Retry plus scheduled
  rescan and watch re-establishment, so a half-watching state cannot
  persist silently. Same model-plus-code-plus-test shape as S3; not
  yet in `requirements.json` (candidate requirement under G1).
- **S5 [BACKLOG]: Audit tool (optional, deferred).** Crawl-vs-rows comparison
  (`watchmanctl audit` pattern) as the operational convergence check.
  Revisit after S3/S4.
- **S6 [DONE]: Lock the goal set and the fairness owner** (2026-11-10).
  G0 committed (narrowed, code anchors, torn reads declared a
  non-goal); G1 committed (conditional on quiescence plus a completed
  scan); G2 queued pending S7; G3 queued pending an inbox model or a
  per-route barrier gate; G4 dropped as a goal (the 404 contract is a
  product decision, kept open). Fairness owner: rescan on the
  watcher's `Flag::Rescan` signal while the watcher is enabled (S3)
  plus a per-root periodic backstop that the user enables at regular
  intervals (`QUINT-ASS-7`).
- **S7 [DONE]: Strategy selection** (2026-11-10). NO-WATCHER is a
  modeling baseline (the scan-only convergence runs), not an
  implementation option; the deployment target is the hybrid: watcher
  fast path, rescan on `Flag::Rescan` while the watcher is enabled,
  and the user-enabled regular backstop (`QUINT-ASS-7`). Convergence
  bound: one backstop interval after quiescence (two scan cycles at
  most). Consequence: blocking mutating API
  during scans (the former NO-WATCHER requirement) is not imposed;
  what remains is scan-vs-assign coordination, since the rescan and
  backstop scans do run concurrently with API mutations.
- **S8 [DONE]: Encode per-root sweep.** Decided: sweep is
  constrained to the scan root. Encoded in
  `specs/eventual-consistency.qnt`: `sweepPending` is keyed by root,
  `ScanRoot` marks the root pending, `SweepRoot` prunes only that
  root; `perRootSweepTest` pins the isolation.
- **S9 [DONE]: Proc-scoped convergence.** Decided: `Converged`
  covers processable files only; unprocessable files are logged, not
  indexed. Encoded: `unprocessable` state, `MarkUnprocessable`,
  `WorkerOpenFails` writes nothing, and both `Converged` and
  `NoPermanentStale` exclude unprocessable paths;
  `failingForeverTest` pins it. The unprocessable-files
  contradiction (formerly REQ-2) is closed at the model level.
- **S10 [DONE]: Derive requirements from committed goals**
  (2026-11-10). 14 open obligations in `specs/requirements.json`
  covering sidecar identity, file and directory rename, per-root
  sweep verification, the rescan signal, the periodic backstop,
  scan-vs-assign coordination, thumbnail checksum, orphan logging,
  tmp sweep, rotate/assign ordering, atomic sidecar writes, and the
  G2 cycle semantics (non-overlapping, drain-complete cycles) that
  make the one-interval bound well defined.
  Each carries an owner and its gate test; the channel is active
  again in `spec-check`/`catalog-check`.
- **Open backlog (no home yet):**
  watcher-error recovery (S4; not yet in `requirements.json`);
  missing-asset serving contract (500 today vs the 404 contract;
  formerly G4 — decide 404/placeholder and derive a requirement);
  expire/interactive lanes in the spec (SCOPE-3 collapses them);
  `NoTornRead` plus worker micro-stages (model v2; the worker stays
  collapsed for now);
  verify walk roots never cover the derived store (ASS-4
  assumed-true, untested — falsifier: a watcher event originating
  from the derived tree);
  Dylint enforcement for the skip guard (revisit with the hash
  short-circuit, if the predicate spreads past its single call site).

## Method

Prove first, impose after (`docs/quint-modeling.md`, Strategy).
The model may lean only on approved assumptions plus the registered
scope delimiters; anything else is a hidden assumption
and blocks the merge. Enforced by `just spec-check` over committed
files (machine-readable `@assumes`/`@requires`/`@scope` tags,
registries in `specs/assumptions.json`, `specs/scope.json` and
`specs/requirements.json`, generated `specs/traceability.json` map) —
node only, deliberately not CI-wired. Requirements are not
reverse-covered by model tags: they derive from committed goals and
are gated by implementation tests. Human judgment
keeps three jobs:
strategy choice, assumption and goal correction, and the soundness
review of each classification.

### Curation loop (how new items enter and stay honest)

1. **Propose**: new ASS/SCOPE with owner, status, and a
   falsifier (ASS) or gate (REQ); speculative items stay out of
   `specs/` until reviewed.
2. **Human review**: approve, reword, or reject. Unapproved content
   never merges — `just spec-check` fails untagged definitions and
   unregistered IDs, but only a human judges truth.
3. **Annotate**: tag the model elements (`@assumes`/`@requires`/`@scope`);
   the generated `specs/traceability.json` map must stay green.
4. **Impose**: a REQ moves `open` → `imposed` → `verified` only with
   a live gate; a gate is code plus a test that fails without it.
5. **Revisit**: every step-back re-examines unowned ASS, gateless
   open REQs, and SCOPE items whose justification no longer holds;
   findings become open questions or new steps, never silent drift.

## Requirements

Derived registry: `specs/requirements.json` (REQ-1..14), all `open`
— obligations derived from committed G0/G1/G2, each with an owner
and the gate test that would impose it. `just catalog-check` enforces
the derivation direction: only committed goals may source a
requirement, and every committed goal must source at least one.
`@requires` tags resolve against the registry again (none are needed
today: the model prescribes the behaviour rather than assuming it).
A requirement moves `open` → `imposed` when its gate test exists and
`verified` when that gate is green. REQ-1 is conditional: the hash
short-circuit is not in the current tree, so rescans reprocess and
clear sidecar-derived state; the requirement covers re-enabling it.
The 2026-11-09 removal stands as history: the earlier registry was
filed before any requirement had model evidence.

## Assumptions

World-given premises live in `specs/assumptions.json` with their
formal elements in `specs/eventual-consistency-assumptions.qnt` — nothing is
restated here. Each entry there carries owner and status; model
refs come from the generated traceability map. This plan tracks only the
open findings (unowned or unverified premises), which live in the
open backlog — not in a shadow registry here.

## Maintenance

Single source rule: `specs/` holds content (assumptions, scope,
catalogs, mapping); this plan tracks status, decisions, and
open questions only. No prose duplication — point at files.

- Update triggers: every status change, every new decision, every
  new open question. Never claim done without the gate green
  (`just spec-check`, `just catalog-check`, `just quint-check`,
  `just events-check` as applicable).
- Review trigger: any ASS without a verified owner gets revisited on
  every step-back; unowned items are findings, never background.
- SCOPE items live in `specs/scope.json` with bound, why_safe, and
  lifting step; a scope item whose justification stops holding
  becomes an open question here.

## Out of scope

- Apalache/TLC, Shuttle, DST, Loom, Kani, Stateright.
- Visual or UX design of the presentation states (states themselves
  are in scope).
- Changing the single-binary, self-contained deployment shape: no
  Watchman daemon or other external services (see brief).
- CI wiring for `just spec-check` (deliberate: just rule only, for
  now — see Method).

## Decisions

- 2026-11-08: S3 and S4 accepted as the first code conclusions;
  audit tool deferred as optional.
- 2026-11-08: No Watchman adoption (C++ daemon breaks the
  self-contained shape); keep the `notify` crate and implement the
  recovery patterns on top of it.
- 2026-11-08: Event catalog lives in `specs/`, validated by a
  dependency-free `.mjs` script. Hazard fixes wait until the overview
  exists, except S3/S4 which the overflow evidence already justifies.
- 2026-11-09: Prove-first method adopted (`docs/quint-modeling.md`
  Strategy): model proves under approved assumptions plus tracked
  strategy requirements; requirements are imposed on the code after.
  Enforced by `just spec-check` (closed-world audit, generated
  traceability, no CI wiring).
- 2026-11-09: REQ-4 (store concurrency) resolved: delegated to redb,
  not tracked or proven — the pipeline model serializes before the
  store. Left with the REQS removal.
- 2026-11-10: Requirements derived and the channel re-enabled:
  14 open obligations in `specs/requirements.json`, sourced only from
  committed goals. `catalog-check` enforces that a requirement derives
  from a committed goal and that every committed goal derives at least
  one requirement; `@requires` tags resolve against the registry again.
  All 14 stay open until their gate tests exist.
- 2026-11-10: G2 committed. N = one backstop interval after
  quiescence (convergence by the end of the first backstop cycle that
  starts after changes stop; two scan cycles at most counting a scan
  already in flight), anchored by `boundedConvergenceTest`; conditional
  on the user-enabled backstop (`QUINT-ASS-7`).
- 2026-11-10: Model hardening. Every run that settles in `Converged`
  now also asserts `NoLostUpload` and `NoPermanentStale`; the skip
  runs pin their intake states; `skipStaleTest` pins the
  FS_MODIFY_INFLIGHT gap (`not(NoPermanentStale)`); the per-root
  sweep runs assert the ghost is tolerated only while its root is
  pending. `boundedConvergenceTest` added as the G2 anchor (one
  backstop interval after quiescence; two scan cycles at most).
- 2026-11-10: S7 done. NO-WATCHER is a modeling baseline only, not
  an implementation option; deployment target is the hybrid (watcher
  fast path + `Flag::Rescan` while the watcher is enabled +
  user-enabled regular backstop). Convergence bound: one backstop
  interval (`QUINT-ASS-7`). The former NO-WATCHER requirement to
  block mutating API during scans is dropped; scan-vs-assign
  coordination remains, because rescans run concurrently with API
  mutations in the hybrid.
- 2026-11-10: S6 done. Goals: G0 committed (narrowed to explicit
  removal/relocation plus atomic auxiliary writes, code-anchored;
  torn-byte reads declared a model non-goal), G1 committed
  (quiescent convergence, `Converged` + the runs), G2 queued (bound
  pending S7), G3 queued (needs an inbox model or per-route barrier
  gate; upload waits on the flush batch), G4 dropped as a goal (the
  missing/in-flight serving contract becomes a product decision).
  Fairness owner: rescan on the watcher's `Flag::Rescan` signal while
  the watcher is enabled (inotify overflow, S3, unimplemented), plus a
  per-root periodic backstop that the user enables at regular
  intervals (`QUINT-ASS-7`, unverified). With the watcher off the
  backstop is the sole trigger; the one-interval bound is conditional
  on the backstop running.
- 2026-11-09: New model `specs/eventual-consistency.qnt` replaces
  `specs/index-pipeline.qnt`: every catalog event maps to an action,
  including recovery (`ScanRoot`/`SweepRoot`, `ProcessCrash`,
  `LoseThumbs`, `DbLoss`), directory rename
  (`ExternalRenameDir`/`ApiAssignDirMove`), file rename as
  remove+create (`ExternalRenameFile`), and sidecar events carrying
  sidecar identity (the S8 prescription). Per-root sweep and
  proc-scoped convergence encoded (S8/S9); bounds grow to five paths
  and two roots; SCOPE-6 (completed-write crash abstraction) and
  SCOPE-7 (flat directory paths) added. 32 runs green under
  `just quint-check`; the requirements channel stays dormant.
- 2026-11-09: Requirements channel removed as premature:
  `specs/REQS.md` and `specs/REQS-NO-WATCHER.md` deleted, no archive;
  `@requires` tags stripped (8 sites) and the five definitions they
  solely grounded re-tagged (`AppSidecarEdit`/`uploadTest`/
  `apiRemoveTest` → SCOPE-5, `ReadMissing` → `@exempt observer`,
  `WorkerOpenFails` → `@exempt no-backing` pending the new model).
  `specs/SCOPE.md` converted to `specs/scope.json` (validated by
  `just catalog-check`); `spec-check` fails any `@requires` tag while
  the channel is dormant. New requirements derive only from the
  proven model.
- 2026-11-09: Assumptions pruned to world-given premises
  (QUINT-ASS-1..6); atomic writes, unprocessable handling,
  read-after-write coherence, and store concurrency promoted to
  REQs; scan exclusivity recorded as QUINT-REQ-NW-1, not an
  assumption. Spec split into state/assumptions/goal/pipeline
  modules composed with native Quint imports.
- 2026-11-09: Per-root sweep and proc-scoped convergence recorded
  as decided; encoding in the model still queued. In-flight drain
  for REQ-NW-1 and REQ-4 linearization scope left as open
  questions, not decisions.
- 2026-11-09: Goals split into proposals (G0 safety floor, G1
  quiescent convergence, G2 bounded, G3 read-after-write,
  graceful-degradation contract) — each needs a commit/queue/drop
  decision plus anchoring in the model (S6) before it constrains
  anything. Fairness ownership recorded as an open gap, not an
  assumed property.
- 2026-11-09: Plan tracks status/decisions/open questions only;
  specs/ hold content (single-source rule). Maintenance triggers:
  update on every status change or new decision/question; review
  every unowned ASS and every gateless open REQ on each step-back.
- 2026-11-09: Plan upleveled from item tracker to procedure tracker:
  Goals/Assumptions/Requirements sections point at specs/ files and
  record only status, owners, and open questions; per-item state
  (including owner/status) lives in the specs annotations. Added
  explicit curation loop (propose → human review → annotate →
  impose → revisit).
- 2026-11-10: Single tracker. `index-protocol-spec.md` retired (its
  completed implementation ladder and superseded contract list are in
  git history); the Dylint note moved to the open backlog. The
  eventual-consistency brief folded into this file, whose Goal and
  Rules now carry its content.
- 2026-11-10: Hash short-circuit removed from this branch. The
  implementation is preserved on `backup/hash-short-circuit`; the
  model's skip actions and REQ-1 remain as the contract for re-landing
  it with sidecar identity. Until then rescans reprocess and clear
  sidecar-derived state, so the sidecar-deletion hole is not in the
  shipped code.
