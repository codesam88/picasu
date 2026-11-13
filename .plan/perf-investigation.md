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

Not perf (kept for separation): main rescan hang — pre-existing
terminal-transition correctness bug, recorded in
`.plan/observer-baseline.md` open items.
