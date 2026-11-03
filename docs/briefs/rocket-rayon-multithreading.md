# Brief: Multithreaded processing with Rocket + Rayon + Tokio — low-latency I/O handlers plus CPU-bound background work (image processing, hashing, thumbnails)

Researched: 2026-11-03 | Versions: {rocket 0.5.2, tokio 1.54.1 full, rayon 1.12.0, tokio-rayon 2.1.0, image 0.25.10 rayon, dashmap 6.2.1 rayon, Rust edition 2024} | Overall confidence: high for Tokio/Rocket/Rayon core patterns; medium for tokio-rayon (last release 2021, limited maintenance) and for image's runtime parallelism controls (compile-time feature only, pool selectable via `ThreadPool::install`)

## 1. Baseline (~30%)

Rocket 0.5 is fully async on a Tokio multi-thread runtime: each request is an async task on a small worker pool (default = logical CPU count, set via `ROCKET_WORKERS` or `Rocket.toml` `workers`; cannot be set programmatically). Handlers are `async fn`; scheduling is cooperative at `.await`. `max_blocking` (default 512) bounds `spawn_blocking`-style threads and, like `workers` and `shutdown.force`, is read only from the default provider (`Config::figment()` / `ROCKET_*`); the default 512 "should not be changed unless physical or virtual resources are scarce" [S3, S5].
Rocket has no `shutdown_timeout` configuration key. Graceful shutdown is controlled by `shutdown.grace`, `shutdown.mercy`, `shutdown.ctrlc`, `shutdown.signals`, and `shutdown.force`. With `force = true` (default), Rocket waits grace + mercy (+ at most 1s) and then forcefully terminates a Rocket-configured async runtime; Rocket never forcefully terminates a custom runtime [S3, S5].

Tokio's two bridging APIs: `tokio::task::spawn_blocking` runs a closure on a large (default 512, on-demand, 10s keep-alive) blocking thread pool suited to blocking I/O, with an unbounded queue and tasks that cannot be aborted once started (abort may prevent a not-yet-started task); `tokio::task::block_in_place` runs on the current worker thread after handing other tasks off, requires the multi-thread runtime, borrows locals, and suspends the rest of the task. Both are uncancellable and block shutdown; `Runtime::shutdown_timeout(self)` consumes the runtime and stops waiting after a timeout (tasks keep running) [S2]. `spawn_blocking` threads that run indefinitely belong on dedicated `std::thread::spawn` instead [S2].

Rayon owns one global work-stealing pool sized to logical cores (configurable via `RAYON_NUM_THREADS` or `ThreadPoolBuilder::build_global`, which succeeds exactly once, before first use). `par_iter`/`join`/`scope`/`spawn` all use the current or global pool. Custom pools via `ThreadPoolBuilder::build()` + `pool.install(|| …)` isolate workloads; Rayon threads live until the pool drops [S4, S6].

tokio-rayon 2.1.0 (released 2021-04-05) bridges the two: global pool via free functions `tokio_rayon::spawn` / `tokio_rayon::spawn_fifo`; custom `rayon::ThreadPool` via `AsyncThreadPool` methods `spawn_async` / `spawn_fifo_async`. Both return `AsyncRayonHandle<R>: Future<Output=R>`, internally a `tokio::sync::oneshot`; the crate re-exports `rayon`. Task panics propagate through the future (no Rayon handler); dropping the handle discards the result but the task may keep running; a panic dropping the return value triggers the pool handler [S1].
The `image` crate's `rayon` feature enables internal multi-threading in some codecs (EXR, AVIF via ravif) and exposes `par_pixels`/`par_enumerate_pixels` helpers. The `dashmap` `rayon` feature adds `par_iter` over maps. Neither exposes a runtime pool selector at the call site; both follow Rayon's current/global pool, but the pool can be scoped with `ThreadPool::install` [S7, S8].

## 2. What research adds (~70%)

**Corrections and updates**

- **Calling `par_iter()` directly inside an async Rocket handler is the canonical mistake.** It blocks the Tokio worker thread for the whole computation; with N workers, N concurrent requests freeze I/O for everyone. One production report (lead): p95/p99 latency spikes to ~2.5s and gateway 504s. Fix is dispatch, not optimism [S9, S10].
- **`spawn_blocking` is the wrong default for CPU work.** Its pool runs up to 512 threads — far more than cores — causing oversubscription and context switching. Tokio docs recommend Rayon (or a semaphore) for many CPU-bound jobs; `spawn_blocking` is for short-lived blocking I/O [S2, S10].
- **`block_in_place` is not a substitute for dispatch.** It still occupies a worker thread for the duration and suspends the rest of the task; it cannot be used in `current_thread` runtimes, and under `join!`/`select!` it suspends same-task siblings. Prefer `spawn_blocking`/Rayon dispatch when the closure can be `'static` [S2, S11].
- **tokio-rayon is convenient but not the only or newest option.** Its last release is 2021; the equivalent manual pattern (`rayon::spawn` + `tokio::sync::oneshot`) is what Alice Ryhl documents and what PostHog used in production in 2026. Both are valid; tokio-rayon removes boilerplate but hides the channel [S1, S9, S10].
- **Rocket has no `shutdown_timeout` key.** Tokio's `Runtime::shutdown_timeout(self)` consumes the runtime and does not apply to a Rocket-owned runtime; Rayon pools are never awaited by Tokio shutdown. Implement an own drain at Rocket's `on_shutdown` fairing hook; use `Runtime::shutdown_timeout` only for runtimes you own [S2, S3, S5].
- **The `block_on` rule is narrower than "never in Rayon".** A panic occurs on re-entrant `block_on` inside an async execution context, not on a mere Rayon thread. A plain Rayon thread that captured a `Handle` can call `Handle::block_on` successfully; `block_on` then `par_iter` with no inner `block_on` is valid (pool-hungry, no panic); `block_in_place` + `Handle::current().block_on` is explicitly supported. The hazard is the sandwich: Rayon -> `block_on` -> async -> `par_iter` -> work-stealing -> a second `block_on` while runtime TLS is still set, producing "Cannot start a runtime from within a runtime". Rule: never call `block_on` on a thread already driving Tokio; prefer channels [S2, S13, S14].- **Panic contracts differ.** tokio-rayon catches a task panic into the future and does not invoke Rayon's panic handler. Bare `rayon::spawn` invokes the `ThreadPoolBuilder` panic handler, which aborts the process by default. A manual `rayon::spawn` + `oneshot` pattern must wrap the task in `std::panic::catch_unwind` to recover `Result` semantics:

  ```rust
  let h = tokio_rayon::spawn(|| work()); // panic surfaces on h.await
  // manual: wrap in catch_unwind for Result semantics
  let (tx, rx) = tokio::sync::oneshot::channel();
  rayon::spawn(move || { let _ = tx.send(std::panic::catch_unwind(|| work())); });
  let r = rx.await.unwrap(); // Ok(value) or Err(Box<dyn Any + Send>)
  ```

  [S1, S4].

- **`build_global` is optional and runs exactly once.** Docs: "not recommended, except … change the default configuration … benchmark". It returns `Err` on a second call; the `RAYON_NUM_THREADS` environment variable is the simpler alternative. A Rocket `liftoff` fairing runs after the socket opens, before serving, and fairings run concurrently, so treat `Err` as "already initialized" and verify with `current_num_threads()` [S4, S6].
- **`std::thread::available_parallelism()` is an estimate, not a direct limit read.** On Linux it scans cgroup v1 mountpoints, reads cgroup v2 `cpu.max` steady-state, overcounts when affinity/cgroup quotas can't be queried (sandboxing), undercounts when the current thread's affinity is pinned, ignores `ulimit` and VM limits, and is recomputed every call (not hot paths). Kubernetes enforces the CPU limit but not the request, so pools over-provision against the request; pin thread counts to the request [std docs].- **`block_in_place` panics only on a `current_thread` runtime**, is allowed outside a runtime (it calls the closure normally), and is allowed under `join!`/`select!` but suspends same-task siblings; prefer `spawn_blocking` when the closure is `'static` [S2].
- **`image`/`dashmap` parallelism is compile-time with no per-op knob**, but scoping selects the pool: wrap `image::open`, `par_pixels`, and `dashmap`'s `par_iter` in `pool.install(|| …)` so codec/map work runs on a chosen pool; otherwise `dashmap`'s `par_iter` follows the current pool [S4, S7, S8].- **Semaphore gating must hold the permit across completion.** Use N permits ≈ Rayon thread count, acquire with `acquire_owned` when the permit moves into the spawned task, and hold it until the oneshot resolves. Dropping the permit at spawn time does not bound the queue [S9, S10].
- **`spawn_blocking` specifics:** 512 default, on-demand threads, 10s keep-alive, unbounded queue, not abortable once started (abort may prevent a not-yet-started task), shutdown waits forever (Drop waits forever), dedicated `std::thread::spawn` for forever-loops; `block_in_place` borrows and suspends the task [S2].
- **Pool budgeting.** Defaults assume each pool owns the machine, so Tokio + Rayon together ≈ 2× cores. Guidance: Tokio workers ≈ cores/2 (mostly parked in `.await`), Rayon ≈ cores, accepting mild oversubscription. Rayon's queue is unbounded, so add a semaphore plus a size threshold (small inputs sequential) [S9].
- **Capture `Handle::current()` before leaving the Tokio context** when using `rayon::spawn` directly [S12, S13].
  **Pitfalls and failure modes**

- **Async↔Rayon sandwich (panic).** Re-entrant `Handle::block_on` on a thread already driving a Tokio runtime panics "Cannot start a runtime from within a runtime." The realistic Rayon case is the sandwich (Rayon → `block_on` → async → `par_iter` → steal → second `block_on` with TLS still set). Never call `block_on` on a thread already driving Tokio; communicate via channels instead [S13, S14].
- **Rayon↔Rayon deadlock via channels.** Two pipeline stages sharing one pool: stage A fills a bounded channel while stage B waits for a free thread to consume it → pool exhaustion deadlock. Use separate pools (`pool.install`) per stage or dedicated threads [S15].
- **`spawn_blocking` threads are not cancellable** and shutdown waits for them; use Rocket's grace/mercy/force (and `Runtime::shutdown_timeout` for owned runtimes). Long-lived loops belong on `std::thread::spawn`, not `spawn_blocking` or Rayon [S2].
- **`block_in_place` panics on `current_thread` runtime** and stalls siblings under `join!`/`select!`. Only use it for borrow-heavy, short, multi-thread-only work [S2, S11].
- **Image crate parallelism is compile-time.** The `rayon` feature toggles internal threading in some codecs; there is no runtime switch for resize/blur ops, and internal parallelism can hurt small images. Treat `image` calls as CPU work that still needs dispatching, and scope them with `pool.install` [S7, S8].
- **Dashmap `par_iter` uses the current/global Rayon pool** — same contention caveats; avoid calling it on hot request paths without dispatch and scoping [S8].

**Alternatives and trade-offs, including when NOT to use Rayon**

- **`spawn_blocking` + semaphore**: simplest, no extra crate; fine for occasional CPU work or I/O. Bad for sustained parallel CPU (oversubscription) [S2, S10].
- **Dedicated `std::thread::spawn` + channels**: right for long-lived workers, persistent DB connections, streaming pipelines; pays thread spawn cost [S2, S10, S15].
- **tokio-rayon / manual `rayon::spawn` + oneshot**: best for CPU-bound request work; fixed core count, work-stealing. Requires `'static` closures and channel handoff [S1, S9, S10].
- **`block_in_place`**: borrow-friendly, multi-thread only, suspends task; use sparingly [S2].
- **`async-threadpool` crate**: wraps a Rayon pool with a `run(|| …)` async API and panic handling; a higher-level alternative if you want the plumbing hidden [S16].
- **When NOT to use Rayon**: blocking I/O; long-lived loops; cross-pool channel dependencies where shared-pool stages deadlock (use per-stage pools); wasm/single-threaded targets without threads; inputs below dispatch threshold; when the `image`/`dashmap` op is already internally parallel and an extra layer only adds scheduling.

**Recommended approach for this context (low-latency I/O + CPU image work)**

1. Keep Rocket handlers async and non-blocking: parse/validate, then dispatch CPU work; `.await` the result. Never call `par_iter`/`image` ops/`hash` directly in the handler [S9, S10].
2. Dispatch via Rayon: prefer tokio-rayon's `spawn`/`spawn_fifo` (global pool) or `pool.spawn_async`/`spawn_fifo_async` (custom pool), or the manual `rayon::spawn` + `tokio::sync::oneshot` pattern; both keep the Tokio worker free. Use `rayon::scope`/`par_iter` _inside_ the Rayon task for data parallelism [S1, S9, S10].
3. Size pools: Tokio workers ≈ cores/2 (or cores if I/O-heavy), Rayon ≈ cores; set both explicitly; in containers pin to CPU request [S9].
4. Gate with a semaphore (permits ≈ Rayon threads, held across the await) and a size threshold; evaluate small inputs sequentially [S9].
5. Do decode/hash/resize/encode entirely inside the Rayon task, scoped with `pool.install` for image/dashmap work; return owned bytes/paths; no `.await` inside [S4, S7, S9].
6. Never `block_on` on a thread already driving Tokio; capture `Handle::current()` beforehand if re-entry is unavoidable, and prefer channels [S12, S13].
7. Use `spawn_blocking` for filesystem I/O and DB work; dedicated threads for forever-loops; `block_in_place` only for short borrow-heavy multi-thread code [S2].
8. Configure Rocket's `shutdown` (grace/mercy/ctrlc/signals/force) and drain Rayon tasks at `on_shutdown`; set `max_blocking` (default 512) deliberately if you use `spawn_blocking` heavily; remember its queue is unbounded [S2, S3, S5].

## 3. Worker checklist

- [ ] Confirm runtime flavor: Rocket uses multi-thread Tokio; `block_in_place`/`Handle::current()` assumptions hold. If tests use `current_thread`, Rayon dispatch still works but `block_in_place` panics.
- [ ] Search handlers for direct `par_iter`, `image::open`, resize/thumbnail/hash calls; move them behind dispatch and scope image/dashmap work with `pool.install`.
- [ ] Decide dispatch mechanism (tokio-rayon global/custom vs manual) and keep one consistent; document channel and panic handling.
- [ ] Set `ROCKET_WORKERS` / `workers` and `RAYON_NUM_THREADS` (or `build_global` in a liftoff fairing, handling `Err` as already-initialized and checking `current_num_threads()`) with a written budget; in containers, pin to CPU request.
- [ ] Add a semaphore gate (permits ≈ Rayon threads, held across the await) and a size threshold; verify backpressure under load.
- [ ] Ensure no `block_on`/runtime construction occurs on a thread already driving Tokio (grep + review); prefer channels.
- [ ] Ensure `image` rayon feature stays enabled; check that codec-internal threading doesn't double-saturate (thresholds and `pool.install`).
- [ ] Configure Rocket `shutdown` (grace/mercy/force) and drain Rayon tasks at `on_shutdown`; for owned runtimes, use `Runtime::shutdown_timeout`. Confirm graceful shutdown drains or abandons them as intended.
- [ ] Validate with a load test: p95/p99 latency and CPU saturation; compare dispatched vs direct.

## 4. Sources

Ranked: primary docs and production experience first.

1. tokio-rayon docs (docs.rs 2.1.0) — `spawn`/`spawn_fifo` (global pool) and `AsyncThreadPool::spawn_async`/`spawn_fifo_async` (custom pool), panic propagation, handle drop semantics; re-exports rayon; uses oneshot internally. https://docs.rs/tokio-rayon/2.1.0/tokio_rayon/ [S1]
2. Tokio docs `spawn_blocking`/`block_in_place` + CPU-bound guide — canonical bridging guidance, 512 default, on-demand, 10s keep-alive, unbounded queue, non-cancellable, current_thread caveat, `block_on` re-entry and sandwich hazard. https://docs.rs/tokio/1.54.1/tokio/task/fn.spawn_blocking.html [S2]
3. Rocket config guide v0.5 (workers/max_blocking/shutdown) — Rocket-specific knobs, default-provider-only limitation, graceful shutdown grace/mercy/ctrlc/signals/force, no `shutdown_timeout` key. https://rocket.rs/guide/v0.5/configuration/ [S3, S5]
4. Rayon docs: `ThreadPoolBuilder`, `build_global`, `install`, `current_num_threads` — sizing, exactly-once global init, isolation primitives. https://docs.rs/rayon/1.12.0/rayon/struct.ThreadPoolBuilder.html [S4]
5. Rayon FAQ (1.12.0) — thread count defaults, work-stealing, non-Send types, `RAYON_NUM_THREADS`. https://docs.rs/crate/rayon/1.12.0/source/FAQ.md [S6]
6. Alice Ryhl, "Async: What is blocking?" (2020) — the canonical Tokio+Rayon+oneshot pattern and the spawn_blocking/Rayon/dedicated-thread decision table. https://ryhl.io/blog/async-what-is-blocking/ [S10]
7. PostHog, "Untangling Tokio and Rayon in production" (2026-04-08) — real latency numbers, pool budgeting, semaphore backpressure, thresholds, container CPU-request pinning. https://posthog.com/blog/untangling-rayon-and-tokio [S9]
8. dureuill, "Naming is hard" / mixing Rayon and Tokio (2024-05-14) — the async Rayon sandwich panic and the "don't block_on in Rayon" takeaway. https://blog.dureuill.net/articles/dont-mix-rayon-tokio/ [S13]
9. subspace PR #2029 + forum thread — concrete backtrace of "Cannot start a runtime from within a runtime" and the fix (enter runtime at spawn level). https://github.com/subspace/subspace/pull/2029 [S14]
10. Daniel Imfeld, "Avoiding Rayon thread pool exhaustion" (2023) — channel/control-flow deadlock across shared pool; per-stage pools. https://imfeld.dev/writing/avoiding_rayon_thread_pool_exhaustion [S15]
11. image crate (0.25.10) Cargo.toml + README + issue #2357 — rayon feature scope, no runtime controls, `ThreadPool::install` scoping. https://docs.rs/crate/image/0.25.10/source/Cargo.toml.orig [S7, S8]
12. dashmap (6.2.1) docs — `rayon` feature and `par_iter` using the current/global pool. https://docs.rs/dashmap/6.2.1/dashmap/ [S8]
13. codeloopback "Blocking and CPU-Bound Work" (2026) — 512 default, semaphore recommendation, rayon-in-spawn_blocking example. https://codeloopback.dev/articles/tokio-blocking-and-cpu-work [S11]
14. users.rust-lang.org "Can rayon and tokio cooperate?" — don't put Rayon workers on Tokio's blocking pool; keep pools separate; capture `Handle::current()`. https://users.rust-lang.org/t/can-rayon-and-tokio-cooperate/85022 [S12]
15. async-threadpool crate (2026) — higher-level Rayon-backed async pool with panic handling. https://github.com/ambient-xyz/async-threadpool [S16]
16. Rocket fairings guide (v0.5) — liftoff runs after socket open, before serving, concurrently; on_shutdown hook. https://rocket.rs/guide/v0.5/fairings/ [S3]
17. Rust std docs: `std::thread::available_parallelism` (1.99.1) — estimate, cgroup v1/v2 behavior, affinity, ulimit, recomputed per call. https://doc.rust-lang.org/std/thread/fn.available_parallelism.html [S17]

Source tags: S1 tokio-rayon; S2 Tokio; S3 Rocket; S4 Rayon; S5 Rocket config; S6 Rayon FAQ; S7 image; S8 image/dashmap; S9 PostHog; S10 Alice Ryhl; S11 codeloopback; S12 users.rust-lang.org; S13 dureuill; S14 subspace; S15 Imfeld; S16 async-threadpool; S17 Rust std. Single-source anecdotes tagged `lead`.
