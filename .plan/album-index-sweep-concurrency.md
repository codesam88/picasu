---
status: open
type: bug
priority: medium
area: backend
---

Concurrency/locking concerns in the stale-alias sweep introduced with the PR \#17 work. Raised in PR \#17 review
(2026-09-07); needs confirming and deciding.

## Context

`sweep_stale_aliases` (`backend/src/tasks/actor/album_index.rs:302-369`) runs at the end of a manual album index. Two
concerns:

1. **Read lock held across FS I/O.** The in-memory tree read lock (`TREE.in_memory.read()`) is held while each candidate
   record does `abs.exists()` checks and possibly `std::fs::remove_file` on thumbnails. For a gallery with millions of
   images this can stall prefetch/readers for the duration of the sweep's disk coverage.

2. **Detached-batch ordering race.** The per-file index tasks enqueue `FlushTreeTask` inserts as detached batches;
   `sweep_stale_aliases` removes/ updates records via `execute_batch_detached`; both are drained afterwards with
   `execute_batch_waiting`. A still-queued per-file insert for a record the sweep just pruned can be applied by the
   drain, re-adding the stale record until the next index. The sweep reads the in-memory tree as it currently stands, so
   its view lags any queued-but-unapplied inserts.

## Tasks / decision

- [ ] Confirm concern 1 magnitude: how long can the sweep hold the lock during `exists()`/`remove_file` on directories
      with many records; whether iteration should snapshot under a short lock and do disk I/O lock-free.
- [ ] Confirm concern 2: trace batch ordering between the per-file tasks, the sweep's detached remove/insert, and the
      post-sweep drain; decide whether the sweep must run after the drain (or drain inserts before sweeping).
- [ ] Sweep currently cascades thumbnail deletion when a record's last alias vanishes (`compressed_path` remove) —
      confirm that is the intended behavior and that multi-root (multi-alias) records are handled (partial prune vs. full
      record removal), with a test if missing.
