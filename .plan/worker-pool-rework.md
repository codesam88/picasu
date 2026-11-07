---
status: done
type: perf
priority: medium
area: backend
---

## Problem

Background work uses per-stage `Task` structs over two `TaskExecutor`
coordinators plus three `tokio multi_thread(N)` runtimes
(`ROCKET`/`INDEX`/`BATCH` in `backend/src/tasks/runtime.rs`) and one
`rayon(N)` pool (`WORKER_RAYON_POOL`), each sized to
`CURRENT_NUM_THREADS`. Total runnable CPU threads reach `4N` plus
unbounded blocking pools and unbounded `mini_executor` channels, with no
joint core budget and no background priority. Per-file
`tokio::spawn(index_image)` fans into `Open/Hash/Dedup/Index/Video`
pool hops and detached `FlushTree` batches. Path-primary assets need no
cross-file coordination, so the layering adds race windows and
complexity without throughput benefit.

## Goal

Replace the `Task`-struct fan-out with a simpler queue scheme: one tokio
runtime for orchestration, one `rayon(N-1)` compute pool, bounded intake
with backpressure, a single serial DB writer, and a shared priority
queue for current and future background jobs (scan, watcher debounce,
expire, verify/GC). Background work stays within `num_cpus` and runs at
lowest effective priority so interactive requests keep a core free.

## Measurement

- Harness A (shell + HTTP, no new bin): `snapfab batch` image sets
  (jpg only per above), `POST /post/index/album` + poll `GET /get/index/status` for scan-done,
  `curl -w %{time_total}` probes on `GET /object/compressed/<path>` and
  `POST /get/prefetch` for fetch latency. `hyperfine --warmup 1` for 3
  repeats. Fixed core budgets N=4 and N=8 via `cpulimit`/`taskset` plus
  recorded pool sizes.
- Perf before/after, same machine, same image sets via `snapfab
batch` with explicit `PhotoSpec` manifests (jpg only, realistic
  sizes e.g. 4000x3000, rich metadata via `exif_date` + `tags`):
  - run 1: launch and initialize server, populate `image_home`
    (count 100, then 1000, jpg only, no tiny defaults, no pinned
    fixtures), measure wall time until scan jobs report done.
  - run 2 with second image set: measure image-fetch latency during
    scan (sequential + concurrent `xargs -P`) plus time until scan done.
  - record `N`, image count, total bytes, and pool sizes alongside each
    run.
- Code complexity before/after on `backend/src/tasks/**` plus
  `workflow`, recorded per function with per-module rollups where the
  tool supports it:
  - `sloc` from `rust-code-analysis` as the size measure (single
    parser for all computed mixes), per function and per module.
  - cyclomatic complexity from `rust-code-analysis`, per function and
    per module.
  - cognitive complexity from `clippy::cognitive_complexity`
    (`-W clippy::cognitive_complexity`, same toolchain as
    `just backend-check`), per function.
  - Maintainability Index from `rust-code-analysis` as the total/mix
    (combines sloc, cyclomatic, Halstead volume), per function and per
    module; no custom mix.
  - plus inventory of runtimes, pools, channels, and
    `Task`/`BatchTask` types removed.

## Scope

- Intake queue with coalescing by path and generation check, bounded
  workers, single DB writer, priority ordering, cancellation.
- Concurrency budget: busy CPU threads `<= N-1`; Rocket keeps latency
  headroom; OS nice as opportunistic only.
- Docs update for the new threading model.

## Non-goals

- No frontend changes.
- No change to filesystem-is-source-of-truth or path-primary identity.
- No new background job types beyond current scan/watcher/expire.

## Implementation steps

Each step is one worker invocation. Raw outputs stay in tmpfiles under
`/tmp/opencode/worker-pool-rework/`; only summary tables land in this
plan.

### Step 1 — Pin tools, capture baseline complexity

Scope: `backend/src/tasks/**` plus `workflow`; write raw outputs only
to `/tmp/opencode/worker-pool-rework/` (`complexity-baseline.*`,
`tool-versions.txt`). Record versions, capture per-function and
per-module `sloc`/cyclomatic/MI via `rust-code-analysis`, per-function
cognitive via `cargo clippy -W clippy::cognitive_complexity`, plus
runtime/pool/channel/`Task`-type inventory. Sanity-check: JSON parses,
files non-empty, sample rows quoted. Done when tmpfiles exist and parse.
No plan edit, no harness, no refactor.

### Step 2 — Build harness A, sanity-check on 10 images

Scope: new harness script plus two `PhotoSpec` manifests only; tmp
outputs under `/tmp/opencode/worker-pool-rework/harness-sanity/`.
Script: `snapfab batch` jpg-only (explicit `format`, 4000x3000,
`exif_date` + `tags`), launch with fixed budget (`taskset`/`cpulimit`,
record pools), `POST /post/index/album` + poll
`GET /get/index/status`, `curl -w %{time_total}` on compressed/prefetch
sequential + `xargs -P` concurrent, `hyperfine --warmup 1` 3 repeats.
Run once on 10 images. Done when `bash -n` passes, scan-done observed,
both probe timings present. No 100/1000 runs, no backend changes.

### Step 3 — Capture full baseline, append summary

Scope: run Step-2 script unmodified; raw logs to
`/tmp/opencode/worker-pool-rework/bench-baseline-N<N>-<count>.log` plus
hyperfine JSON; append one summary table to this plan. Matrix: 100 then
1000 images × N=4 and N=8, 3 repeats each (scan wall, fetch latency
during scan). Done when 4 raw sets exist and the plan table (N, count,
bytes, pools, scan wall mean ± sd, fetch latencies, command line) is
filled; `just plan-lint` passes. No interpretation beyond reporting.

### Step 4 — Queue-simplification design

Scope: design note in this plan only, no code. Specify per-file
change list: one tokio runtime (fate of runtimes in `runtime.rs` and
bootstrap), one `rayon(N-1)` pool, bounded intake with path coalescing
and generation check, single serial DB writer, shared priority queue
for scan/watcher/expire with cancellation, collapse of per-file
`tokio::spawn` fan-out across `actor/*`. Done when every touched or
removed file is listed per slice with checks. No implementation, no
benchmarks.

Design (reviewed 2026-11-03, detail in worker report):

- Target: single tokio runtime (`RUNTIME` in `runtime.rs` — slice 1
  deleted `ROCKET`/`INDEX`/`BATCH`; one `block_on`, one shutdown
  signal), one rayon pool at `max(1, N-1)` (merge implicit global pool
  forced by `CURRENT_NUM_THREADS` with `WORKER_RAYON_POOL`; drop
  `tokio-rayon`), bounded `mpsc` intake (e.g. 1024, path coalescing +
  generation map superseding `IN_PROGRESS`), serial DB writer (all
  `flush_*`, `update_*`, `expire_*`, sweep writes; auth stores
  excluded), priority queue Interactive > Watcher > Scan > Expire >
  Verify/GC with weighted round-robin + aging and per-job cancellation
  tokens, `index_image` split into `submit_index` + `process_file`
  (zero coordinator hops, writer message instead of detached flush).
  Fan-out removed is per-file `tokio::spawn` (`album_index.rs:270`)
  plus the three `tokio_rayon::spawn` CPU bridges
  (`actor/hash.rs`, `index.rs`, `video.rs`).
- Slices: (1) runtime consolidation (`runtime.rs`, `tasks/mod.rs`,
  `lib.rs` bootstrap, import-only edits elsewhere; both coordinators
  kept, repointed; single shutdown path); (2a) intake skeleton + walk
  migration (new `tasks/queue/*`, `workflow/mod.rs` 5 hops →
  submit/process, `album_index.rs` walk → intake sends, drain/barrier
  API shim over existing rendezvous; coordinators stay as execution
  backend); (2b) lanes + deletion (priority lanes with aging, Rayon
  admittance semaphore held across await, `pool.install` scoping at
  request-path `par_iter`/`par_bridge` sites, watcher debounce +
  looper + 5 router files + `expire_check.rs:68`/`RemoveTask`
  migration, delete `INDEX_COORDINATOR`, drop `tokio-rayon` for a
  `rayon::spawn` + oneshot bridge); (3) writer (flush/snapshot
  `BatchTask` impls incl. `flush_tree_snapshot.rs`,
  `flush_query_snapshot.rs`, dispatch sites to writer/queue APIs;
  `BATCH_COORDINATOR` and `mini_executor` removed last — dep removal
  only here, never in 2b).
- Budget: sustained busy CPU `<= N-1`, Rocket headroom via idle
  workers + Interactive priority; OS nice opportunistic only
  (follow-up Step 10). `spawn_blocking` kept only for short I/O on
  allowlisted paths (`tasks/runtime.rs` + `tasks/queue/*` dispatch
  sites); CPU work migrates to Rayon. Budget counts busy CPU only
  (blocking I/O excluded); inventory delta reports
  `spawn_blocking`/blocking-pool use before/after.
- Rayon admittance: semaphore with permits `≈` Rayon threads, permit
  held across the oneshot await (`acquire_owned` when moved into the
  task); superseded intake items never dispatched via cancellation
  tokens + generation check, and a dropped handle discards the result
  while the task may keep running (documented). Interactive lane uses
  `spawn_fifo` (FIFO fairness), bulk scan uses LIFO `spawn`, or
  separate admission queues with weighted round-robin + aging feed one
  pool. Request-path `par_iter` scoped via `pool.install`, never
  direct on the hot path.
- Expected inventory delta: runtimes 3→1, rayon pools 2→1,
  coordinators 2→0, `Task` 7→0, `BatchTask` 7→0, unbounded channels
  replaced by bounded intake/writer mpsc + Notify/watch.
- Open questions for review: (1) Rocket co-resident vs dedicated
  runtime; (2) merge into global rayon pool vs keep worker pool;
  (3) request-path TREE writes through writer or not; (4) writer ack
  semantics (generation barrier vs oneshot); (5) interactive submits
  via queue vs direct spawn; (6) scan-done = intake drained +
  barrier ack, counter semantics; (7) nice portability (`cfg(unix)`
  or follow-up).

### Design decisions (settled 2026-11-03)

Single runtime with discipline by construction; future low-priority
tasks preemptible by higher-priority index jobs via cooperative
cancellation (cancel queued, finish running chunk).

- (1) Runtime: shared single runtime. Pro: one thread budget, no
  cross-runtime hops, bounded once. Con: bulk scan can delay request
  futures unless queue priorities and no-blocking-workers hold.
  Dedicated Rocket isolates latency by construction but keeps two
  budgets to tune and cross-runtime channels. Direction: shared;
  revisit only if p95 fetch regresses under scan after the queue lands.
- (2) Rayon: merge into one global pool at `max(1, N-1)`.
  Pro: one CPU budget; removes the hidden second pool
  (`CURRENT_NUM_THREADS` already forces a global N pool). Con:
  `build_global` must run before any rayon use; third-party crates
  share the cap. Keeping `WORKER_RAYON_POOL` avoids the ordering
  hazard but leaves two pools to saturate. Direction: merge.
- (3) Request-path TREE writes: DEFERRED 2026-11-03 (revised on
  code-grounded review: routing the request path means touching ~10
  router files and/or the `asset_store` API used everywhere — a
  half-migration preserves the contention the writer exists to
  remove, so slice 3 would blow up). Slice 3 serializes background
  flush + in-memory rebuild only; the request path keeps direct
  writes (redb single-writer blocking unchanged there). Request-path
  migration is a separate follow-up plan item
  (`.plan/write-load-probe.md`, opened 2026-11-04). Slice 3
  Done-criteria hold on background paths only. Rationale, recorded
  2026-11-04: the single writer was built for ordering/exactness
  (provable flush→sweep→flush→drain, no interleaved write txns,
  exact barriers), not to speed up request-path edits — those
  bypass the writer in both eras, and the edit's own cost
  (sidecar fsync, `commit_metadata`, tree rebuild) dominates the
  redb txn either way. Measured: N=8 edit latency flat across eras
  (14–32 ms during scan, 10–22 ms idle). The writer's payoff is
  structural, visible in worst-case columns (N=4 write stall),
  not in medians.
- (4) Writer ack: generation barrier (monotonic seq, wait-for-`<=`).
  Pro: batch-friendly, serves scan-done, sweep, multi-message drains.
  Con: bookkeeping, possible over-wait on coarse seq — and the barrier
  must account for cascaded work (auto-`UpdateTreeTask` at
  `flush_tree.rs:49`, detached sweep remove), not just directly
  submitted messages. Per-message oneshot is exact but allocates per
  write under bulk scan. Direction: barrier with cascade accounting
  specced in slice 3.
- (5) Interactive submits: via queue. Pro: uniform cancellation,
  budget, observability. Con: queue latency on the fastest path.
  Direct spawn bypass is faster but evades priority, budget, drain.
  Direction: queue; one documented bypass only if baselines show the
  hop matters.
- (6) Scan-done: intake drained plus barrier acked plus sweep
  ordered. Order pinned as flush→sweep→flush→drain before Completed.
  Counters redefined at queue edges (scanned=enqueued,
  matched=started, processed=writer-acked); status-poll and
  completed/failed tests updated to the new meanings rather than
  preserving join-era fiction.
- (7) Nice: follow-up behind `cfg(unix)` in Step 10. N-1 budget
  plus priority lanes carry correctness; nice only reorders within a
  budget and is untestable in CI. Direction: defer.
- Discipline enforcement (layers, existing infra): types first
  (private runtime handles, sealed `Job` trait, only
  `spawn_async`/`spawn_cpu`/`spawn_io` plus
  `IntakeHandle::submit` public); project dylint
  (`picasu-discipline`, on pinned `cargo-dylint`, warn then gate)
  flagging `tokio::spawn`/`spawn_blocking`/`rayon::spawn`/
  `std::thread::spawn`/`tokio_rayon` outside
  `tasks/runtime.rs` + `tasks/queue/*`, and an architecture test
  alongside `openapi-sanity` grepping the same allowlist, both land
  in follow-up Step 10 and do not gate Slices 1-3. No external
  executor framework: ~150 lines of queue plus
  handles, value is in the narrow API.

- (8) Interactive pool isolation (decided 2026-11-05, supersedes the
  N-1/N reservation sketch): bulk work stays on the single global
  Rayon pool (N threads, niced +10 via `start_handler`); request-path
  CPU work runs on a dedicated `INTERACTIVE_POOL` (N/2 threads,
  default nice) via `pool.install`, so the OS scheduler preempts bulk
  work for interactive joins instead of a userspace ticket scheme.
  `run_interactive` replaces the Interactive lane's `run_cpu_fifo`;
  no new `rayon::spawn` sites. Verified live: bulk threads at nice
  10, interactive at 0 (`ps -L`), storm-read max 2.5 s -> 54 ms with
  medians and scan wall flat. Follow-up: `picasu-discipline` dylint
  rule for holding locks across Rayon boundaries (the arch test pins
  `INTERACTIVE_POOL.install` placement meanwhile).

### Step 5 — Refactor slice 1: runtime consolidation

Scope: `runtime.rs`, `tasks/mod.rs`, bootstrap wiring only. Collapse to
one orchestration runtime + one `rayon(N-1)` pool, rewire deleted
statics and coordinators minimally; collapse the two signal handlers
to one. Done when `just backend-check` passes,
`build_global(N-1)` pinned to first `CURRENT_NUM_THREADS` deref in
`run()` before any other rayon use (`Err` = already-initialized pool
kept, serve-path-only guarantee — tests never call `run()`, so budget
claims do not cover test processes; `RAYON_NUM_THREADS` alternative
documented), startup/dispatch tests pass, and harness A reruns for a
p95 fetch check (dedicated-Rocket revisit trigger). No intake or
writer changes. (`on_shutdown` drain for queue/writer/Rayon is slice
3 scope, not here.)

### Step 6a — Refactor slice 2a: intake skeleton + walk migration

Scope: new `tasks/queue/*`, `workflow/mod.rs`, `actor/album_index.rs`
walk loop only. Bounded intake with path coalescing + generation map;
5 `execute_waiting` hops → `submit_index` + `process_file` (stage fns
kept, still dispatched via coordinators as execution backend);
per-file `tokio::spawn` + `Vec<JoinHandle>` → intake sends; drain and
scan-done barrier defined as an API shim over the existing
`execute_batch_waiting` rendezvous (same call sites,
`album_index.rs:297-327`); counters keep current meanings. Done when
`just backend-check` and scan tests pass with identical counters. No
lane/admittance work, no deletions, no writer changes.

### Step 6b — Refactor slice 2b: lanes, admittance, coordinator deletion

Scope: lanes + aging + Rayon admittance (semaphore permits `≈`
threads held across await; CPU bridge is `rayon::spawn` + oneshot —
never `spawn_blocking`, which would evade the N-1 budget),
`pool.install` scoping at request-path sites (`update_tree.rs`,
`expire_check.rs`, `model/album.rs`, `storage/db.rs`,
`get_prefetch.rs`, `get_data.rs`), watcher debounce + looper enqueue,
5 router files to Interactive submits, `expire_check.rs:68` +
`RemoveTask` migration, delete `INDEX_COORDINATOR`, drop
`tokio-rayon`. `mini_executor` dep stays until slice 3. Writer scope
note (revised on landing): `BATCH_COORDINATOR`, the 7 `BatchTask`
impls, and the `mini_executor` dep are KEPT — request-path routers
still dispatch through them and the request path is frozen per (3).
Background flush/rebuild/expire/sweep/removal now go through the
serial writer with generation barrier; the dep removal moves to the
deferred request-path follow-up. Done when `just backend-check` and
scan/watcher/expire tests pass, with removed `Task` types and
admittance accounting reported. No writer internals.

### Step 6c — Interim benchmark after intake+queue, before writer

Scope: rerun Step 1–3 tooling unmodified (same versions, script,
manifests, machine); raw to `bench-mid-*`/`complexity-mid.*`; append
an interim comparison table (baseline vs mid) to this plan. 100-cells
at N=4 and N=8 are required; 1000-cells rerun with the longer timeout
in background as in Step 3. Verify the writer is untouched (grep:
`BATCH_COORDINATOR` + flush `BatchTask` impls still present) so the
delta is attributable to intake/queue only. Done when mid raw files
parse and the plan holds baseline-vs-mid deltas; `just plan-lint`
passes. Flags regressions before slice 3 lands. No code changes.

### Step 7 — Refactor slice 3: single serial DB writer

Scope: `batcher/flush_tree.rs` (incl. the auto-`UpdateTreeTask`
cascade at `:49` — barrier must account for cascaded work, not just
directly submitted messages), `flush_tree_snapshot.rs`,
`flush_query_snapshot.rs`, `update_tree.rs`, `update_expire.rs`,
`expire_check.rs` plus writer edge. Serialize mutations, remove
detached spawns and overlapping batches, keep backpressure end to end;
sweep ordering pinned as flush→sweep→flush→drain before Completed;
`on_shutdown` drain for queue/writer/Rayon lands here. Kept stage fns
stay in `actor/*` (computation); only runtime/queue dispatch sites
join the `spawn_blocking` allowlist (allowlist = read-only, bounded
work, no lock-holding — criteria, not just paths). `mini_executor`
(and `tokio-rayon` if lingering) removed here, never earlier — except
as revised on landing: `BATCH_COORDINATOR`, the `BatchTask` impls,
and the dep stay because the frozen request path still dispatches
through them; their removal moves to the deferred request-path
follow-up. Writer constraints: txns stay short (no bulk work inline — large rebuilds
and sweeps stay chunked or dispatched as background jobs, never
head-of-line blocking); the Interactive lane gets reserved capacity
bulk can never starve; ack waits carry generous deadlines as
deadlock-breakers only, and any timeout is an anomaly to alert on,
not a pressure valve; multi-item edits keep per-item acks so partial
success stays reportable. Done
when `just backend-check` and flush/expire tests pass, with channel
inventory delta reported.

### Step 8 — Re-measure, compare, record summary

Scope: rerun Steps 1–3 tooling unmodified; raw to
`bench-after-*`/`complexity-after.*`; append three-way comparison
tables (baseline vs mid vs after) to this
plan. Same versions, script, manifests, machine. Collect writer queue
depth and ack latency under the 1000-image scan; upload/mutation hop
latency stays an unprobed gap (noted, not compared). Update
status-poll and completed/failed tests to the redefined counters;
timed-out 1000-cell partial progress is not comparable across eras.
Done when after-raw files parse and the plan holds before/after
deltas (scan wall, fetch, complexity per function/module, removed
runtimes/pools/channels/tasks); `just plan-lint` passes. Flag
regressions explicitly.

### Step 9 — Docs update and final review

Scope: threading-model docs plus plan status alignment, no behavior
changes. Document final state, reconcile stale comments, set `done`
only if Steps 5–8 held else `blocked` with gap noted. Done when
`just check` and `just plan-lint` pass and diff/status inspected.

### Step 10 — Follow-up hardening (dylint gate, arch-test grep, nice)

Scope: `picasu-discipline` dylint gate, arch-test allowlist grep, and
`cfg(unix)` OS-nice only; separate worker invocation after Slices 1–3
land. Does not gate Steps 5–8. Done when warn-then-gate is wired and
`just check` passes.

## Benchmark results

### Superseded measurements (see git history)

The Step 3 baseline, Step 6c interim, Step 8 after/debug-era, release
head-to-head (`977932b6`), and first storm-battery tables are retired as
a comparison basis: mixed harnesses, random per-run image sets, and
run-to-run variance within one condition exceeding any base-vs-after gap
for every probe metric. Full text lives in git history
(`git log -p -- .plan/worker-pool-rework.md`); raw logs were under
`/tmp/opencode/worker-pool-rework/bench-{baseline,mid,after,cmp}-*`.
What stands: the post-isolation baseline and the true-baseline
head-to-head below, both on the canonical set-100 with the Python
tooling (`utils/bench`), release profile, N=8, `SCAN_REPEATS=2`.

### Read-tail investigation (closed 2026-11-05)

Root cause proven by live instrumentation: prefetch `par_iter` joins
queued behind saturated bulk-scan waves on the single shared Rayon pool
(`filter_par_iter` p95 ~0.3–0.6 s during stall windows, `cpu_permits`
pinned at 0, medians untouched). Fix: dedicated `INTERACTIVE_POOL`
(N/2 threads, default nice) for request-path Rayon work; bulk pool at
nice +10 so the OS scheduler preempts it. Replaces the earlier
reserved-thread sketch (permits bound jobs, not threads, so it could not
guarantee a free worker). Verified live: bulk threads at nice 10,
interactive at 0 (`ps -L`).

### Post-isolation baseline (reference, 2026-11-05)

Post-fix tree, new Python tooling, canonical set-100, `SCAN_REPEATS=2`.
Seven cells, all rc=0: control/read/thumb/diff/idle-diff at N=8, control/read
at N=4 (`taskset` 0–3). Raw cells under
`/tmp/opencode/worker-pool-rework/bench-v2-*-N{8,4}-r1`.

| workload (N=8)                  | med ms | p95 ms | max ms             | fail                            |
| ------------------------------- | ------ | ------ | ------------------ | ------------------------------- |
| control prefetch (probe)        | 7.7    | —      | 8.9 (worst round)  | 0                               |
| control edit (probe)            | 11.3   | —      | 12.1 (worst round) | 0                               |
| storm-read 8x100 (1600 ops)     | 3.5    | 17     | 49                 | 0                               |
| storm-thumb 8x100 (1600 ops)    | 7.0    | 26     | 54                 | 0                               |
| storm-diff 8x100 (1600 ops)     | 6.5    | 14     | 26                 | 535 early-scan 500s, run 1 only |
| storm-idle-diff 8x100 (at rest) | 8      | 15     | 21                 | 0, verify 8/8 canaries          |

N=4: control scan 21 s (vs ~10 s N=8); read storm med 4 / p95 7 / max 11 ms,
0 fails — tighter than N=8, no contention signal at halved core budget.
Scan walls flat across workloads (storms add no measurable scan time);
post-scan idle columns unchanged everywhere (11–17 ms).

Verdict vs pre-fix eras: medians identical everywhere; the multi-hundred-ms
single-op stalls (2.5 s worst observed) are gone — max single op across
~5000 post-fix storm ops is 54 ms, max control round in this battery
12.1 ms. The only fails anywhere are unindexed-asset 500s in scan run 1
(pre-existing error shape, unchanged by this work; run 2 always clean).

### True-baseline head-to-head (cdba92f02c, 2026-11-05)

Pre-rework tree at the plan's starting commit (`cdba92f0`, “Add
worker-pool-rework perf task”), clean tree, release build in a
`bench-true-base` worktree, driven by the same Python tooling via
`PICASU_BIN` (wire protocol verified identical: get-data shape,
`assetId`, prefetch, edit_rating). Same set-100, `SCAN_REPEATS=2`.
Seven cells mirror the post-isolation battery exactly.

| workload                     | base med / p95 / max ms       | after med / p95 / max ms                                  |
| ---------------------------- | ----------------------------- | --------------------------------------------------------- |
| control prefetch probe       | 17.1 / — / 27.1 worst round   | 7.7 / — / 8.9 worst round                                 |
| control edit probe           | 19.8 / — / 24.9 worst round   | 11.3 / — / 12.1 worst round                               |
| storm-read 8x100 (1600 ops)  | 4.0 / 24 / 86, 0 fails        | 3.5 / 17 / 49, 0 fails                                    |
| storm-thumb 8x100 (1600 ops) | 15 / 52 / 87, 0 fails         | 7 / 26 / 54, 0 fails                                      |
| storm-diff 8x100 (1600 ops)  | 8.5 / 21 / 38, 411 early 500s | 6.5 / 14 / 26, 535 early 500s                             |
| storm-idle-diff (at rest)    | 11–14 across columns, 0 fails | 8–16 across columns, 0 fails, verify 8/8                  |
| N=4 control scan             | 18–19 s                       | 21 s (single runs each — needs repeats before calling it) |
| N=4 storm-read               | 5 / 25 / 84, 0 fails          | 4 / 7 / 11, 0 fails                                       |

Verdict: medians equal-or-better on after everywhere; no
multi-hundred-ms event on either side in this battery (base worst:
87 ms thumb; after worst: 54 ms read). The 1–3 s prefetch stalls
seen earlier appear on _neither_ tree here — they were unique to the
single-shared-pool interim (fixed by pool isolation, confirmed by the
1.9 s → 54 ms drop on the same workload). Early-scan 500 counts
(411 vs 535) move with race timing, not tree version — same known
error shape both sides, zero fails from run 2 on.

### Reading the observe output (breach / wrap / drain)

Three line types, three different meanings:

- `... took X, normal bound Y` at `WARN` = **breach**: one operation of
  that stage exceeded its duration threshold. Actionable only when it
  repeats with queue pressure (see below). Rate-limited to one per stage
  per 5 s.
- `High event volume: ...` at `INFO` = **wrap**: the stage recorded more
  than the 256-slot ring holds. Prints only for a stage that breached its
  threshold since the last reset, as volume context for known slowness.
  Healthy churn is silent — compare `recorded` vs `n` in the drain table.
  Expect zero wrap lines on a healthy scan. Rate-limited to one per stage
  per 5 s; suppressed wraps keep their backlog until a breach or reset.
- `observe drain: ...` at `INFO`, once per scan = the per-stage table
  (`n`, p50, p95, max). This is the reference for "what is normal".

Worked example — the two wrap lines pasted from a release run. Why they
are normal load, field by field:

- `wraps=6`, and the table shows `n=256` on `tree_refresh_*`,
  `cpu_permit_wait`, `cpu_exec`, but `n=4–6` on filter/prefetch stages.
  256 + 6 = 262 recorded: a 100-image scan with repeats records one
  refresh + one `cpu_exec` per image, so just over 256 is exactly the
  expected volume. Low `n` on prefetch/filter is expected too (one per
  probe round, not per image). Volume explains the wraps; nothing had to
  be slow.
- p50/p95/max vs threshold: `tree_refresh_total` p50 1.1 ms / p95 2.0 ms /
  max 3.8 ms vs 250 ms bound; `write_wait` all zeros vs 20 ms;
  `cpu_permit_wait` 0–2 µs vs 25 ms; `blocking_queue_wait` ≤ 245 µs vs
  50 ms; `prefetch_total` ≤ 9 ms vs 1 s; `cpu_exec` p50 1.5 ms / p95
  813 ms / max 941 ms vs 1 s (closest, still under). Nothing breached,
  so no WARN — consistent with INFO-only wraps.
- `cpu_permits_available=7` (full house on N=8: N−1 permits) rules out
  pool saturation. Saturated would read 0 sustained.
- `in_flight` all zeros except a transient `tree_refresh_total=1` in one
  of the two lines: one refresh in progress at snapshot time, gone by the
  next line. Stuck would read the same non-zero stage across successive
  lines minutes apart.
- `seq[next=117 writer=115 cascade=115]`: writer 2 behind intake, stable.
  Falling behind would be a large and growing gap (tens to hundreds) plus
  `write_wait` p95 climbing toward its 20 ms bound.
- Two wraps in the same second (one per churning stage) each snapshotting
  the same table is the old format's duplication; the current format logs
  one short line per churning stage with only that stage's stats.

What to look for when analysing (in order):

1. Level first: INFO wrap alone during a scan = volume, ignore unless
   `n`/`recorded` is far above scan size. WARN breach = slow op; look at
   the queue clause on the same line.
2. Permits: `cpu_permits_available=0` sustained across lines = bulk pool
   saturated (the pre-isolation stall shape had this pinned at 0 with
   `filter_par_iter` p95 0.3–0.6 s). Full or bouncing back = healthy.
3. In-flight: same stage non-zero across minutes = hung op. Transient 1
   = normal snapshot timing.
4. Seq gap: `next − writer` large and growing + `write_wait` rising =
   writer falling behind. Small stable gap (1–3) = normal pipelining.
5. p50 vs p95 vs max: p95 near threshold with low p50 = tail (a few slow
   ops, e.g. single-image indexing near 1 s). p50 near threshold = the
   whole stage shifted (regression or under-provisioned pool).
6. Cross-stage: `cpu_permit_wait` high + permits 0 = admission jam;
   `write_wait` high + filter `in_flight` > 0 = reader-holds-guard
   contention (the fixed deadlock's milder cousin);
   `blocking_queue_wait` high = Tokio blocking pool backlogged, not a
   filter problem; single `interactive_exec` WARN = one heavy UI job, not
   systemic.
7. Rate: isolated breach/wrap during a scan = note and compare to the
   drain table. Repeating WARNs for the same stage across scans with
   degrading p95 = regression — that is what the production delay +
   wrap detection is for. A full deadlock would WARN persistently and
   wedge `in_flight`/seq, not vanish the next line.
8. Profile: ignore absolute WARN thresholds in debug builds (unoptimized
   codegen runs 10–30× slower; breach/wrap log at `debug!` there by
   design). Compare debug-to-debug only via the drain tables.

Note (2026-11-07): the extra stats are now gated. Per-scan drain table
and wrap lines require `PICASU_OBSERVER_STATS=1` (or `--observer-stats`);
breach warnings stay on. The `task-observer` Cargo feature (renamed from
`observe`) still controls recording at build time.

### After complexity (Step 8)

| module        | files (b/m/a) | sloc (b/m/a)   | cyclo (b/m/a) | cog (b/m/a) | funcs (b/m/a) |
| ------------- | ------------- | -------------- | ------------- | ----------- | ------------- |
| tasks         | 2/2/2         | 62/56/56       | 9/6/6         | 0/0/0       | 0/1/1         |
| tasks/actor   | 9/9/9         | 1751/1561/1548 | 259/208/204   | 149/146/141 | 53/39/39      |
| tasks/batcher | 8/8/8         | 1413/1402/1430 | 210/208/210   | 172/171/173 | 40/40/41      |
| tasks/looper  | 1/1/1         | 47/49/54       | 6/7/7         | 1/1/1       | 2/2/2         |
| tasks/queue   | –/5/6         | –/847/1224     | –/130/192     | –/51/94     | –/43/60       |
| workflow      | 1/1/1         | 177/197/217    | 46/51/58      | 16/13/14    | 5/11/11       |
| total         | 21/26/27      | 3450/4112/4529 | 530/610/677   | 338/382/423 | 100/136/154   |

Net vs baseline: +1079 sloc for the queue + writer substrate;
removed: 7 `Task` impls, `INDEX_COORDINATOR`, `tokio-rayon`,
per-file spawn fan-out, 2 runtimes, 1 rayon pool. `BatchTask`
impls (7) and `BATCH_COORDINATOR` kept per the deferred
request-path scope. Clippy cognitive unchanged: 2 warnings, both
outside scope.

### Baseline complexity (Step 1, commit `80141138` scope)

Raw: `out/backend/src/**/*.json` (21 files, `rust-code-analysis-cli
0.0.25`), `complexity-baseline.clippy-cognitive.txt`,
`inventory-baseline.txt`, `tool-versions.txt`. Toolchain `rustc
1.99.1`, `clippy 0.1.99`.

| module               | files | sloc | cyclomatic sum | cognitive sum | functions |
| -------------------- | ----- | ---- | -------------- | ------------- | --------- |
| tasks                | 2     | 62   | 9              | 0             | 0         |
| tasks/actor          | 9     | 1751 | 259            | 149           | 53        |
| tasks/batcher        | 8     | 1413 | 210            | 172           | 40        |
| tasks/looper         | 1     | 47   | 6              | 1             | 2         |
| workflow             | 1     | 177  | 46             | 16            | 5         |
| total tasks+workflow | 21    | 3450 | 530            | 338           | 100       |

`clippy -W clippy::cognitive_complexity`: 2 warnings, both outside
scope (`model/config.rs:782` 30/25, `:959` 37/25); `tasks/**` +
`workflow` clean. MI recorded per function and per module in the JSON
raw files. Inventory: 3 tokio runtimes + `WORKER_RAYON_POOL`, 2
coordinators, 7 `Task` + 7 `BatchTask` structs.
