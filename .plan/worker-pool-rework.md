---
status: in-progress
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

- Target: single tokio runtime (`ROCKET_RUNTIME` survives, `INDEX` +
  `BATCH` deleted; one `block_on`, one shutdown signal), one rayon
  pool at `max(1, N-1)` (merge implicit global pool forced by
  `CURRENT_NUM_THREADS` with `WORKER_RAYON_POOL`; drop `tokio-rayon`),
  bounded `mpsc` intake (e.g. 1024, path coalescing + generation map
  superseding `IN_PROGRESS`), serial DB writer (all `flush_*`,
  `update_*`, `expire_*`, sweep writes; auth stores excluded),
  priority queue Interactive > Watcher > Scan > Expire > Verify/GC
  with weighted round-robin + aging and per-job cancellation tokens,
  `index_image` split into `submit_index` + `process_file` (zero
  coordinator hops, writer message instead of detached flush).
- Slices: (1) runtime consolidation (`runtime.rs`, `tasks/mod.rs`,
  `lib.rs` bootstrap, import-only edits elsewhere; both coordinators
  kept, repointed); (2) intake + queue (new `tasks/queue/*`,
  `workflow/mod.rs`, delete 7 `Task` structs keeping stage fns,
  walk sends to intake, watcher/looper enqueue; `INDEX_COORDINATOR`
  deleted); (3) writer (flush/snapshot/update/expire `BatchTask`
  impls deleted, dispatch sites to writer/queue APIs;
  `BATCH_COORDINATOR` and `mini_executor` removed).
- Budget: sustained busy CPU `<= N-1`, Rocket headroom via idle
  workers + Interactive priority; OS nice opportunistic only.
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
- (3) Request-path TREE writes: route through the serial writer on
  the Interactive lane. Pro: single write order, no redb writer
  contention. Con: more call-site churn plus one queue hop. Leaving
  them direct is a smaller diff but preserves the contention the
  writer exists to remove. Direction: through writer.
- (4) Writer ack: generation barrier (monotonic seq, wait-for-`<=`).
  Pro: batch-friendly, serves scan-done, sweep, multi-message drains.
  Con: bookkeeping, possible over-wait on coarse seq. Per-message
  oneshot is exact but allocates per write under bulk scan.
  Direction: barrier.
- (5) Interactive submits: via queue. Pro: uniform cancellation,
  budget, observability. Con: queue latency on the fastest path.
  Direct spawn bypass is faster but evades priority, budget, drain.
  Direction: queue; one documented bypass only if baselines show the
  hop matters.
- (6) Scan-done: intake drained plus barrier acked. Counters
  redefined at queue edges (scanned=enqueued, matched=started,
  processed=writer-acked); status-poll tests updated to the new
  meanings rather than preserving join-era fiction.
- (7) Nice: follow-up behind `cfg(unix)`. N-1 budget plus priority
  lanes carry correctness; nice only reorders within a budget and is
  untestable in CI. Direction: defer.
- Discipline enforcement (layers, existing infra): types first
  (private runtime handles, sealed `Job` trait, only
  `spawn_async`/`spawn_cpu`/`spawn_io` plus
  `IntakeHandle::submit` public); project dylint
  (`picasu-discipline`, on pinned `cargo-dylint`, warn then gate)
  flagging `tokio::spawn`/`spawn_blocking`/`rayon::spawn`/
  `std::thread::spawn`/`tokio_rayon` outside
  `tasks/runtime.rs` + `tasks/queue/*`; architecture test alongside
  `openapi-sanity` grepping the same allowlist for a fast local
  signal. No external executor framework: ~150 lines of queue plus
  handles, value is in the narrow API.

### Step 5 — Refactor slice 1: runtime consolidation

Scope: `runtime.rs`, `tasks/mod.rs`, bootstrap wiring only. Collapse to
one orchestration runtime + one `rayon(N-1)` pool, rewire deleted
statics and coordinators minimally. Done when `just backend-check`
passes and startup/dispatch tests pass. No intake or writer changes.

### Step 6 — Refactor slice 2: bounded intake and priority queue

Scope: `tasks/actor/*`, batcher dispatch surface, `looper/mod.rs` per
Step-4 list. Replace per-file spawn fan-out with bounded intake and
shared priority queue with cancellation. Done when `just backend-check`
and scan/watcher/expire tests pass, with removed `Task`/`BatchTask`
types reported. No writer internals.

### Step 7 — Refactor slice 3: single serial DB writer

Scope: `batcher/flush_tree.rs`, snapshots, `update_tree.rs`,
`update_expire.rs` plus writer edge. Serialize mutations, remove
detached spawns and overlapping batches, keep backpressure end to end.
Done when `just backend-check` and flush/expire tests pass, with
channel inventory delta reported.

### Step 8 — Re-measure, compare, record summary

Scope: rerun Steps 1–3 tooling unmodified; raw to
`bench-after-*`/`complexity-after.*`; append comparison tables to this
plan. Same versions, script, manifests, machine. Done when after-raw
files parse and the plan holds before/after deltas (scan wall, fetch,
complexity per function/module, removed runtimes/pools/channels/tasks);
`just plan-lint` passes. Flag regressions explicitly.

### Step 9 — Docs update and final review

Scope: threading-model docs plus plan status alignment, no behavior
changes. Document final state, reconcile stale comments, set `done`
only if Steps 5–8 held else `blocked` with gap noted. Done when
`just check` and `just plan-lint` pass and diff/status inspected.

## Benchmark results

### Baseline perf (Step 3, harness A, 2026-11-03)

Raw: `/tmp/opencode/worker-pool-rework/bench-baseline-*/`
(`harness-a.log`, `status-final.json`, `hyperfine.json`,
`probe-*.tsv`). Harness `62fe7b23`, `hyperfine 2.0.0`, `taskset`
budgets (no `cpulimit`). Generation dominates: snapfab 4000x3000
jpg at ~7.7s/image (100 in ~13min).

| N   | count | bytes    | pools                              | scan wall         | fetch seq mean | fetch conc mean   | hyperfine compressed | hyperfine prefetch |
| --- | ----- | -------- | ---------------------------------- | ----------------- | -------------- | ----------------- | -------------------- | ------------------ |
| 4   | 100   | 46690163 | tokio 4x3, rayon 4, srv-threads 21 | 421s (1949 polls) | 0.0030s (n=99) | 0.0046s P4 (n=99) | 0.00928s             | 0.00831s           |
| 8   | 100   | 62329256 | tokio 8x3, rayon 8, srv-threads 37 | 228s (977 polls)  | 0.0042s (n=99) | 0.0108s P8 (n=99) | 0.01544s             | 0.01368s           |

100-cells: `completed`, `failed=0`, 100/100/100. 1000-cells hit the
30min `SCAN_TIMEOUT_S` with zero failures (N4: 416/1000, N8:
773/1000 processed) — scan rate ~4.2s/image at N4, ~2.3s/image at
N8 on 4000x3000 jpg. 1000-cell full completion to be rerun with a
longer timeout; numbers above are the committed baseline for
before/after comparison.

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
