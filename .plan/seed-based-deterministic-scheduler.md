---
status: idea
type: feature
priority: medium
area: testing
---

Async batch tasks (`UpdateExpireTask`, `FlushTreeTask`, etc.) are fire-and-forget via `BATCH_COORDINATOR.execute_batch_detached()`. In tests, the multi-threaded `BATCH_RUNTIME` processes these concurrently with the test thread — the timing is non-deterministic, causing flaky failures that are unreproducible.

**Failure case:** `assign_album` moves a child album and must invalidate the prefetch cache synchronously. Without a synchronous `VERSION_COUNT_TIMESTAMP` bump before the handler returns, the cache retains stale data. But the async `UpdateExpireTask` (which does bump the timestamp) may or may not have run by the time the test issues a prefetch — the test passes or fails based on scheduler luck.

**Proposed solution:** A seed-based deterministic scheduler (`DeterministicScheduler`) that replaces `BATCH_RUNTIME` / `TaskExecutor` in `#[cfg(test)]` builds. `execute_batch_detached` queues tasks but defers execution; the test controls when each queued task runs via a schedule of `(task_seq, delay_ms)` entries. For random exploration the schedule is generated pseudo-randomly from a seed (different seeds → different interleavings). For debugging, a specific failing seed is replayed verbatim. No source-code barriers needed.

- what are the chances of actually catching races with this?
  it seems rather coarse-grained based on defined tasks
- I expect we have to rework the task management system anyway,
  should wait and maybe take this into account.
