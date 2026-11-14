---
status: open
type: chore
priority: medium
area: backend
---

## Goal

Single index of performance-investigation directions, so focused work
starts from one list instead of scattered plan residuals. The
investigations target the index pipeline as restructured by the impose
work (sweep, leases, cascade, view invalidation) — the recorded
baselines measure that pipeline end to end. Each item carries its measured context and points at the task that owns deeper
work. Measurement infrastructure already exists (Python bench stack,
observer instrumentation) — new items should state what to measure
before proposing changes.

## Related homes (do not duplicate here)

- `.plan/impose-requirements.md` — source of the pipeline changes these
  items build on: C9 batched DB sweep, C10 per-path leases/abort
  baseline, C11 cascade rebuild outside write txns, decoupled view
  invalidation (`de841f67`), upload store-and-return. Every item below
  traces to its work or its recorded residuals (C6 follow-ups, C9
  large-set deferral, query-scoped-view optimisation note).
- `.plan/worker-pool-rework.md` (done) — first benchmark setup and
  baseline perf: harness A → canonical Python cells, pre-rework
  true-baseline head-to-head (`cdba92f0`), read-tail investigation
  (dedicated `INTERACTIVE_POOL`), storm-battery protocol, and the live
  instrumentation that seeded the observer framework.
- `.plan/observer-verbosity-gating.md` — shaped the observer's current
  surface: `task-observer` feature, `PICASU_OBSERVER_STATS=1` +
  `--observer-stats` gating (breach warnings always on).
- `.plan/bench-harness-py.md` — measurement tooling (launcher + workload
  wrapper + observer `PICASU_OBSERVER_STATS=1` protocol).
- `.plan/fs-db-architecture-review.md` — structural FS/DB ops review,
  O(1) album moves; the right place for schema-level O() redesigns.
- `.plan/frontend-grid-benchmark.md` — frontend-perceived grid lag.
- `.plan/observer-baseline.md` — recorded baselines: main vs impose
  pre-fix vs post-fix (`de841f67`), 2000-image rig. Post-fix loaded
  reads: p50 27 ms, p95 130 ms, max 171 ms; edit_tag 102 ms.
- `.plan/quint-asset-aspects.md` — mutation-window modeling, not perf.

## Phase B roadmap (bench/work, added 2026-11-14)

Done before this roadmap: Phase A harness fixes (wall-clock schema,
margin default, extractor wordings, hyperfine required);
`IMAGE_HOME_SRC` sandbox override + comparable run manifests;
observer stats default-on in bench mode (distortion A/B: no signal,
run variance dominates); `docs/benchmarks.md` protocol.

Standing rule from the A/B: single cells carry no comparison (3x wall
variance back-to-back on one machine). Batteries repeat cells and
compare medians across cells, not within one.

- **S1. Cheap materialisation + large-file archive.** `copy2` per cell
  becomes link-or-copy (hardlink same fs, copy fallback); sandbox mode
  (`IMAGE_HOME_SRC`) skips materialisation and records reuse. Seed one
  large-file archive class (few hundred real-camera-sized files).
  Files: `workload.py`, `seed_images.py`, tests. Acceptance: set-100
  cell byte-identical results via link path; large-file repo seeds and
  scans. Enables S5.
- **S2. Drain-table parsing.** Extractor parses `observe drain:` tables
  from the cell `server.log` into per-scan stage rows joined by scan
  index to request latencies. Files: `extract-bench.py`, tests with
  recorded server.log fixtures. Acceptance: re-parse of the Phase A
  validation cell yields stage rows matching the manual read. Enables
  S4/S5 analysis.
- **S3. Upload profile.** New `WORKLOAD=upload`: submit files, record
  submit-to-200 plus time-to-visible per file, verify background drain.
  Files: `workload.py`, tests, README. Acceptance: upload cell at
  set-100 green with populated upload columns. Independent of S1/S2.
- **S4. Battery runner + checked-in baselines.** Pinned env per battery
  (profiles x scales x load), one result row per cell, baselines file
  with bands; a cell outside its band fails until explained and
  re-recorded. Files: new `run-battery` recipe/doc, `baselines.tsv`
  (location decided at build). Needs S2. Acceptance: set-100 gate
  battery green against fresh baselines from repeated cells.
- **S5. Large-set rig.** Seed 5650 archive once, run control + storm at
  scale, record; promote to 50k+ only when 5650 is clean. Needs S1+S4.
  Acceptance: scale numbers recorded with stage attribution; items 1-4
  re-examined against them.
- **S6. Tuning on evidence only.** Items 1-4/6-8 promote to task files
  only with a triggering cell + metric attached. Stop rule holds.

Order: S1, S2, S3 (any order, S3 independent), then S4, then S5, then S6.

## Items (added 2026-11-13)

1. **Tree rebuild is O(rows) and the leading-edge miss pays it in full.**
   Post-fix, one read per ~1 s throttle window rebuilds the whole tree:
   14–48 ms at 2000 rows. At the millions-of-images target (~10–25 µs/row
   measured) a rebuild far exceeds the freshness window and leading-edge
   reads become multi-second. Solve space: incremental/streaming rebuild,
   serve-previous-snapshot-while-rebuilding, query-scoped view (item 4).
   Needs a scaled rig (item 5) before choosing.
2. **Rebuild-cost discrepancy unexplained.** Same 2000-row library:
   main 454 ms build vs impose 14–48 ms (~10–30×). The earlier
   "0.23 ms/row identical on both branches" record does not extrapolate.
   Root-cause before trusting any scaling extrapolation from old
   measurements.
3. **`filter_snapshot_clone` is O(rows) per cache miss.** 8–127 ms at
   2003 rows under rescan CPU contention; co-dominant with rebuild in
   pre-fix measurements. Cascade already shares snapshots via `Arc`
   (C11); the read-path clone does not. Sharing/COW across reads is the
   candidate direction.
4. **Query-scoped view / full redb scan+sort on the requesting thread.**
   Open optimisation recorded in impose review notes; overlaps item 1's
   solve space.
5. **Large-set benchmark effort.** C9 residual + 3a decision: scale the
   verify rig (5650-row library, then 50k+) with observer
   instrumentation; define bounded-latency targets for scan, loaded
   reads, and edits at scale. Prerequisite for deciding items 1/3/4.
6. **Edit-path slowness at scale.** Fast at 2000 rows (102 ms), slow at
   5650 rows pre-fix on both branches (barrier/flush path). Unresolved
   at larger libraries; item 5's rig re-measures it.
7. **C6 follow-ups: fsync policy + read-modify-write LWW.**
   Durability/latency tradeoff, recorded in the impose plan; not
   measured yet.
8. **REQ-1/REQ-8 skip optimization.** Perf-adjacent (skip rescans of
   unchanged assets); deferred with its own registry entries in
   `utils/quint/pipeline/requirements.json`.
9. **`temp_db.redb` growth under probe load (added 2026-11-14).**
   After identical 5650 scans, `temp_db.redb` ended at 2.0 GiB apparent
   in the control cell (141 hyperfine/prefetch probe rounds plus
   edit-rating probes) vs 8.4 MB in storm-diff (no probe rounds).
   Dominates per-cell disk. Worth a look before 50k: what the probe
   path writes that the storm path does not.

Not perf (kept for separation): main rescan hang — pre-existing
terminal-transition correctness bug, recorded in
`.plan/observer-baseline.md` open items.
