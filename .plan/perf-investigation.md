---
status: open
type: chore
priority: medium
area: backend
---

## Goal

Single index of performance-investigation directions, so focused work starts
from one list instead of scattered plan residuals. The investigations target
the index pipeline as restructured by the impose work (sweep, leases, cascade,
view invalidation) — the recorded baselines measure that pipeline end to end.
Each item carries its measured context and points at the task that owns deeper
work. Measurement infrastructure already exists (Python bench stack, observer
instrumentation) — new items should state what to measure before proposing
changes.

## Related homes (do not duplicate here)

- `.plan/impose-requirements.md` — source of the pipeline changes these items
  build on: C9 batched DB sweep, C10 per-path leases/abort baseline, C11
  cascade rebuild outside write txns, decoupled view invalidation
  (`de841f67`), upload store-and-return. Every item below traces to its work
  or its recorded residuals (C6 follow-ups, C9 large-set deferral,
  query-scoped-view optimisation note).
- `.plan/worker-pool-rework.md` (done) — first benchmark setup and baseline
  perf: harness A → canonical Python cells, pre-rework true-baseline
  head-to-head (`cdba92f0`), read-tail investigation (dedicated
  `INTERACTIVE_POOL`), storm-battery protocol, and the live instrumentation
  that seeded the observer framework.
- `.plan/observer-verbosity-gating.md` — shaped the observer's current
  surface: `task-observer` feature, `PICASU_OBSERVER_STATS=1` +
  `--observer-stats` gating (breach warnings always on).
- `.plan/bench-harness-py.md` — measurement tooling (launcher + workload
  wrapper + observer `PICASU_OBSERVER_STATS=1` protocol).
- `.plan/fs-db-architecture-review.md` — structural FS/DB ops review, O(1)
  album moves; the right place for schema-level O() redesigns.
- `.plan/frontend-grid-benchmark.md` — frontend-perceived grid lag.
- `.plan/observer-baseline.md` — recorded baselines: main vs impose pre-fix vs
  post-fix (`de841f67`), 2000-image rig. Post-fix loaded reads: p50 27 ms, p95
  130 ms, max 171 ms; edit_tag 102 ms.
- `.plan/quint-asset-aspects.md` — mutation-window modeling, not perf.

## Phase B roadmap (bench/work, added 2026-11-14)

Done before this roadmap: Phase A harness fixes (wall-clock schema, margin
default, extractor wordings, hyperfine required); `IMAGE_HOME_SRC` sandbox
override + comparable run manifests; observer stats default-on in bench mode
(distortion A/B: no signal, run variance dominates); `docs/benchmarks.md`
protocol.

Standing rule from the A/B: single cells carry no comparison (3x wall variance
back-to-back on one machine). Batteries repeat cells and compare medians
across cells, not within one.

- **S1. Cheap materialisation + large-file archive.** `copy2` per cell becomes
  link-or-copy (hardlink same fs, copy fallback); sandbox mode
  (`IMAGE_HOME_SRC`) skips materialisation and records reuse. Seed one
  large-file archive class (few hundred real-camera-sized files). Files:
  `workload.py`, `seed_images.py`, tests. Acceptance: set-100 cell
  byte-identical results via link path; large-file repo seeds and scans.
  Enables S5.
- **S2. Drain-table parsing.** Extractor parses `observe drain:` tables from
  the cell `server.log` into per-scan stage rows joined by scan index to
  request latencies. Files: `extract-bench.py`, tests with recorded server.log
  fixtures. Acceptance: re-parse of the Phase A validation cell yields stage
  rows matching the manual read. Enables S4/S5 analysis.
- **S3. Upload profile.** New `WORKLOAD=upload`: submit files, record
  submit-to-200 plus time-to-visible per file, verify background drain. Files:
  `workload.py`, tests, README. Acceptance: upload cell at set-100 green with
  populated upload columns. Independent of S1/S2.
- **S4. Battery runner + checked-in baselines.** Pinned env per battery
  (profiles x scales x load), one result row per cell, baselines file with
  bands; a cell outside its band fails until explained and re-recorded. Files:
  new `run-battery` recipe/doc, `baselines.tsv` (location decided at build).
  Needs S2. Acceptance: set-100 gate battery green against fresh baselines
  from repeated cells.
- **S5. Large-set rig.** Seed 5650 archive once, run control + storm at scale,
  record; promote to 50k+ only when 5650 is clean. Needs S1+S4. Acceptance:
  scale numbers recorded with stage attribution; items 1-4 re-examined against
  them.
- **S6. Tuning on evidence only.** Items 1-4/6-8 promote to task files only
  with a triggering cell + metric attached. Stop rule holds.

Order: S1, S2, S3 (any order, S3 independent), then S4, then S5, then S6.

**Run hygiene (2026-11-17).** Long or timed commands write full output to
`/tmp/opencode/perf-p1-retention-scale/<step>-<cmd>-<ts>.log` plus a `.exit`
code, and are inspected from that file; never rely on a live pipe's tail,
because a timeout then leaves no evidence. `just backend-test` hung twice in
`random_op_sequences_converge` (the logged rerun aborted in
`cancel_and_wait_index_idle` after 864 s: index job never idle, scanned=0)
while the test alone passes in ~7 s and the full suite passed twice afterwards
(56.5 s / 58.5 s); treat a repeat as interference until a rerun reproduces it.
Check `/proc/cpuinfo` MHz before trusting absolute bands: the 2026-11-17 gate
run sat at 900 MHz (`powersave`) and missed the idle bands by ~1.8x while the
base/current A/B stayed within 0.4 ms.

## Items (added 2026-11-13)

1. **Tree rebuild is O(rows) and the leading-edge miss pays it in full.**
   Post-fix, one read per ~1 s throttle window rebuilds the whole tree: 14–48
   ms at 2000 rows. At the millions-of-images target (~10–25 µs/row measured)
   a rebuild far exceeds the freshness window and leading-edge reads become
   multi-second. Solve space: incremental/streaming rebuild,
   serve-previous-snapshot-while-rebuilding, query-scoped view (item 4). Needs
   a scaled rig (item 5) before choosing. **Blocked (2026-11-17
   verification).** `04fc9d18` is not merged into the retention branch:
   Playwright duplicate-files fails 5/5 and the quint check drifts against the
   spec. The measured result below stands; the change is parked until those
   failures are resolved. **Leading-edge miss resolved** in `04fc9d18`:
   staleness is now classified as background-only or request-path. A reader
   whose view was built before only background writes keeps the published view
   and the rebuild runs on the bulk pool — single-flight, `try_lock` (never
   blocking a reader), one rebuild per freshness window, retrying until it
   catches up. Request-path bumps and scan terminal transitions still rebuild
   synchronously; the watcher's removal lookup keeps a forced-fresh read. The
   prefetch query key includes the view generation, so a result computed from
   the served-stale view is a miss after the rebuild (without it the cached
   result kept the view stale indefinitely). Micro-measured (release,
   synthetic `ASSET_BY_ID` rows, 20 iterations after warmup, median/p95):
   request-path stale read 4.64/5.91 ms at 2000 rows and 9.20/15.57 ms at 5650
   rows, versus 79 ns / 65 ns for the background-stale read — the fresh-read
   floor. The O(rows) rebuild itself is unchanged, so incremental rebuild and
   the query-scoped view stay open, and item 5's scale rig is still needed
   before scaling claims. End-to-end prefetch p95/max under a scan was not
   measured (set-100 gate cell is below resolution for this stage).
2. **Rebuild-cost discrepancy unexplained.** Same 2000-row library: main 454
   ms build vs impose 14–48 ms (~10–30×). The earlier "0.23 ms/row identical
   on both branches" record does not extrapolate. Root-cause before trusting
   any scaling extrapolation from old measurements. **Instrumented**
   (2026-11-17): the extractor and battery report now carry every rebuild term
   (`tree_refresh_total/build/sort/write_wait`) and the filter path
   (`filter_snapshot_clone`, `filter_par_iter`, `compute_locate`,
   `prefetch_total`) as per-scan p50/max columns, so a rebuild can be
   attributed to iteration, sort, or the write guard instead of one aggregate.
   Row sizes are pinned for the memory model: `DatabaseTimestamp` 584 B,
   `ReducedData` 168 B (test + `observe rows:` line). A 5650-row scale cell
   with this breakdown is the next evidence; the root cause stays open until
   it runs.
3. **`filter_snapshot_clone` is O(rows) per cache miss.** 8–127 ms at 2003
   rows under rescan CPU contention; co-dominant with rebuild in pre-fix
   measurements. Cascade already shares snapshots via `Arc` (C11); the
   read-path clone does not. Sharing/COW across reads is the candidate
   direction. **Resolved** in `d0499623`: the tree view is published as
   `Arc<Vec<DatabaseTimestamp>>` and the prefetch filter takes
   `snapshot_tree_arc()` (O(1)); tag counting uses the same handle.
   Micro-measured release clone cost on synthetic rows (path + 2 tags + 10
   EXIF fields, 20 iterations after warmup): 7.1 ms at 2000 rows and 16.7 ms
   at 5650 rows (3.6 / 3.0 µs per row) versus 25–26 ns for the `Arc` clone.
   The 2000-row figure matches this item's 8 ms floor. End-to-end
   re-measurement is still open: the gate set-100 cell is below resolution for
   this stage and the 2000-row rig needs a bench cell.
4. **Query-scoped view / full redb scan+sort on the requesting thread.** Open
   optimisation recorded in impose review notes; overlaps item 1's solve
   space. The lazy-fetch direction for items 1/3/4 waits on the deferred
   option table under item 9.
5. **Large-set benchmark effort.** C9 residual + 3a decision: scale the verify
   rig (5650-row library, then 50k+) with observer instrumentation; define
   bounded-latency targets for scan, loaded reads, and edits at scale.
   Prerequisite for deciding items 1/3/4. The rig now records, per cell: every
   rebuild and filter stage as p50/max, exact `snapshot_memory_hit` /
   `snapshot_disk_fallback` counts, peak RSS (`vm_hwm_mb`) and `temp_db.redb`
   size (`temp_db_mb`). **Scale result (2026-11-17, release, set-5650, 3 reps,
   host at 900 MHz powersave).** `storm-diff` completed: scan wall
   median 524.6 s (prior single cell 648 s; host state differs), `temp_db` 8.4
   MB / 8 tables in every rep (prior cell identical), VmHWM 448 MB. Stage p50
   medians: `filter_snapshot_clone` 53.6 ms, `filter_par_iter` 25.9 ms,
   `tree_refresh_build` 35.7 ms, `tree_refresh_total` 53.6 ms,
   `prefetch_total` 83.0 ms. `control` did not complete: two runs froze with
   the writer/cascade sequences stuck and all worker threads idle (counters
   `(5391,5390,4357)` and `(4887,4885,3852)`), after the during-scan
   `/get/prefetch` probe timed out (>30 s). Logs: `step2-scale-prefix-*.log`
   and the cell `diag-orchestrator/stacks.txt`. No 50k seed exists; at the
   5650 set's 3.1 GB it is ~27 GB of images, and a 524 s scan extrapolates to
   ~77 min/scan. Not attempted. **Control retry 2026-11-19 (fixed tree
   `619e392b`, serial, default cap 1024, 1000MHz powersave):** completed rc=0,
   no stall. Scan walls 503.0/488.6s median 495.8s, 5738/5650/5650, final
   next/writer/cascade 14800/14799/14799, max next-writer lag 9, max
   writer-cascade lag 1453 (exceeds cap — under review, not a freeze), VmHWM
   1371MB, temp_db 1078MB, hit 396 / fallback 3160, scan prefetch p50 9.0ms
   p95 123.5ms max 163.6ms, no Unknown id. Log
   `20261119-023807-5650-control-retry.log`, cell
   `5650-control-retry/cell-control-rep1`. Unblocks retention delta. **Gate
   (set-100) A/B:** base `d0499623` vs this branch, two control rounds each,
   idle p50 within 0.4 ms (prefetch 6.9/7.0 vs 7.3/7.1, thumb 7.4/7.8
   vs 7.6/7.6, edit 7.5/7.4 vs 7.8/7.5). The absolute gate battery missed
   `baselines.tsv` by ~1.8x on idle edit/prefetch while `/proc/cpuinfo`
   reported 900 MHz (governor `powersave`, no turbo); the A/B shows no code
   effect, so the bands are not re-recorded.
6. **Edit-path slowness at scale.** Fast at 2000 rows (102 ms), slow at 5650
   rows pre-fix on both branches (barrier/flush path). Unresolved at larger
   libraries; item 5's rig re-measures it.
7. **C6 follow-ups: fsync policy + read-modify-write LWW.** Durability/latency
   tradeoff, recorded in the impose plan; not measured yet.
8. **REQ-1/REQ-8 skip optimization.** Perf-adjacent (skip rescans of unchanged
   assets); deferred with its own registry entries in
   `utils/quint/pipeline/requirements.json`.
9. **`temp_db.redb` growth under probe load (added 2026-11-14).** After
   identical 5650 scans, `temp_db.redb` ended at 2.0 GiB apparent in the
   control cell (141 hyperfine/prefetch probe rounds plus edit-rating probes)
   vs 8.4 MB in storm-diff (no probe rounds). Dominates per-cell disk. Worth a
   look before 50k: what the probe path writes that the storm path does not.
   **Cause** (measured 2026-11-17 on the surviving 5650 control cell): one
   whole-view table per prefetch cache miss. That cell held 2006 tree tables
   / 8.46M rows / 1.4 GiB for 998 probe requests — the probe prefetch runs
   after the round's edit moved the tree version, so it misses, and a miss
   wrote the entire 5653-row view to its own redb table. Tables are deleted
   only when the version they belong to expires (~1 h), so the steady state is
   one table per miss in the last hour, not a function of the storm path.
   **Partial fix** (`ebc51874`, view-cache eviction bound): the newest views
   stay in memory under `PICASU_TREE_SNAPSHOT_MEMORY_MB` (default 64 MiB) and
   only overflow is written to disk, which takes the resident budget out of
   the steady-state disk size. On 200 miss-per- round rounds (5653 rows): 201
   tables / 269 MB before, 131 tables / 135 MB at the default budget, 0 at 512
   MiB. It does not touch the retention term, which is what scales with miss
   rate. **Open:** snapshot retention is the ~1 h query-cache expiry, while a
   prefetch token is valid for 300 s. Shortening tree-table retention to the
   token lifetime (or deleting a table when its last referencing query cache
   row is replaced) is the lever for the 2 GiB. **Scale re-check
   (2026-11-17):** the probe-free `storm-diff` cell still ends at 8 tables
   / 8.4 MB (both before and after the P1 base), so it does not exercise this
   item. The `control` cell that produces the growth could not complete twice
   (cascade stall, item 5), so the post-retention delta at 5650 is unmeasured;
   the surviving pre-fix cell is 2006 tables / 2,155,876,352 B. Retention is
   unchanged in this branch. **Deferred (2026-11-17).** `ebc51874` (P3,
   view-cache eviction bound) is deferred pending a verdict on the lazy-fetch
   options below. No further `temp_db` fixes until the option (b) verdict.

   | option | shape                                          |
   | ------ | ---------------------------------------------- |
   | a1     | ordinal list + pinned `Arc` snapshot           |
   | a2     | id list, no pin                                |
   | b      | query-scoped range scan, no snapshot           |
   | c      | virtual scrollbar buckets computed at prefetch |

   Analysis basis: scroll is an absolute-index model (a page implies a fixed
   row offset), the view is frozen for the 300 s prefetch-token lifetime, auth
   is evaluated at token mint rather than per read, and no date-ordered index
   exists.

Not perf (kept for separation): main rescan hang — pre-existing
terminal-transition correctness bug, recorded in `.plan/observer-baseline.md`
open items.

10. **Cache-DB write-slot stall (found 2026-11-18, P1-only).** 5650 `control`
    freezes with `writer-cascade = cap+1` (1025 @1024, 129 @128) while cascade
    blocks in `expire_check_task:27` `take_write_slot` and batch
    `ExpireCheckTask:34` holds the `QUERY_SNAPSHOT` write txn across
    `par_bridge` to `:98`; `flush_query_snapshot:39` waits on the same slot
    holding the DashMap shard guard (`:28`), prefetch `get_prefetch.rs:302`
    waits on the shard. No `try_begin_write` in redb 4.4.0; batch bodies park
    `tokio-core`; `par_bridge` from non-pool needs bulk worker. Small-cap
    repro: 2000 images cap 128 freezes 178.5s lag 129, same triple frames;
    serial single cell, no concurrent cells. Agreed sequence: (i) txn/guard
    probe + cascade op+seq line (zero behavior, permanent), (ii) minimal
    ordering (split sweep, drop guard before begin_write), (iii) 2a single
    sweeper (`UpdateExpire` batch → `WriteOp::ExpireCheck`); defer 2b full
    single-writer until (ii) numbers show residual waits. Landed (i) in
    `e13100ba`, (ii) in `9990c108` (incl. keep-until-commit). Small-cap
    2000-image cap-128 re-run green: scan 2002/2000/2000, lag max 3 (<129),
    4527 `/get/data` 200 with no Unknown snapshot id, VmHWM 585.9MB,
    temp_db 142.6MB. 2a single-sweeper first tried 2026-11-19 under background
    load and reverted (+143% scan wall, lag 230 — host-contaminated, 303s vs
    546s variance). Idle re-run 2026-11-19 clean: before 188.4s vs
    after 175.3s (-7%, lag max 2-3 both, VmHWM -6.5%, temp_db same, exit 0
    both, 945→1100MHz drift noted). 2a reverted before PR (partial
    single-writer only; sweep-vs-sweep waiter gone but flush paths still
    direct, so it adds a special case for a noise-range delta). Stall fix
    stands on ordering + keep-until-commit; single-writer per DB stays an open
    design item (2b skipped). 2a no-regression evidence (2000-img control,
    cap-128, serial, 945→1100MHz drift; reports `cell-2a-before2/report.tsv` /
    `cell-2a-after2/report.tsv`, logs `1795050986` / `1795051562` exit 0; both
    stamp base `563f7219`, after carried uncommitted 2a diff):

    | metric                            | before                  | after                   |
    | --------------------------------- | ----------------------- | ----------------------- |
    | scan_wall_s                       | 188.353                 | 175.258                 |
    | idle prefetch p50/p95/max ms      | 7.2/7.2/7.2             | 7.1/7.3/7.3             |
    | idle thumb                        | 7.8/8.0/8.1             | 7.9/8.0/8.0             |
    | idle edit                         | 7.7/7.8/7.8             | 7.7/7.8/7.8             |
    | scan prefetch                     | 12.4/51.1/70.7          | 8.2/47.1/65.3           |
    | scan thumb                        | 10.2/19.5/25.4          | 3.2/8.6/18.0            |
    | scan edit                         | 11.0/13.5/20.0          | 8.6/11.0/14.0           |
    | build/clone/par_iter/total p50 µs | 13951/18611/25171/18605 | 13257/17475/24892/17470 |
    | build/clone/par_iter/total max µs | 34322/46308/64978/46302 | 19946/27175/50743/27170 |
    | hit/fallback                      | 62/997                  | 77/1065                 |
    | vm_hwm_mb / temp_db_mb            | 577.9/134.7             | 540.2/134.7             |

    (`prefetch_total` p50 39816→52 µs excluded as sparse-sample artifact
    vs 8.2ms scan prefetch; uscan/storm/upload n/a on control.) 2b full
    single-writer per cache DB skipped 2026-11-19 (effort M-L for robustness
    only; current lag ≤3 needs no queue). Residual: per-DB owner +
    guard-across-txn class still open by design, not by omission.
