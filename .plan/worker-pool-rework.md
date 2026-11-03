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

Raw logs live under `/tmp/opencode/worker-pool-rework/`; this section
holds summary tables only. Baseline table (Step 3) and before/after
comparison (Step 8) to be appended by those steps with per-cell N,
count, bytes, pools, scan wall mean ± sd, and fetch latencies.
