---
status: in-progress
type: bug
priority: high
area: backend
---

## Notes

Concurrency review of the album-delta-aggregates series (`18171596` and
surrounding model-driven-invariant commits, 2026-11-15) found one
deadlock pattern at three call sites plus related races/stalls. All
findings are on `origin/main`; none was introduced by the perf
branches. Evidence: gdb thread dumps of the CI hang
(`random_op_sequences_converge`, 4/11 local repro) plus a full-diff
review of `18171596` with spot verification of critical cites.

Dominant lock convention: `DIR_ALBUM_CACHE` mutex is acquired before
`TREE.in_disk.begin_write/read` (`process/dir_album.rs:201,280`,
`init_dir_album_cache`). The bug sites invert this.

### Fix now (this branch)

- **A. ABBA at flush site (CI flake).** `flush_tables` holds the
  `TREE.in_disk` write txn (begun `tasks/batcher/flush_tree.rs:155`,
  commit `:361`) and locks `DIR_ALBUM_CACHE` via
  `ancestor_album_ids_for_path` (`:348` → `process/dir_album.rs:176`).
  Concurrent `get_or_create_dir_album` holds that mutex across
  `begin_write` (`process/dir_album.rs:201-280`) → cycle. Observed
  hang: `album index did not complete within 30s` +
  `cancel_and_wait_index_idle` 90s timeout; stacks pre/post identical.
- **1. Same inversion, two more sites.** `process/transitor.rs:65`
  (`begin_write`) held across `ancestor_album_ids_for_path` (`:125`);
  `router/put/assign_album.rs:422` (`begin_write`) held across
  `get_parent_album_id` (`:427`, locks) and cover refresh before
  commit (`:439`).
- **7. Reset not fenced.** `tests/bootstrap.rs:268`
  `reset_backend_state` wipes `METADATA_TABLE` and asset tables in two
  txns and clears `DIR_ALBUM_CACHE`, but only quiesces the album index
  (`:272`); writer/cascade/intake queues are not drained, so an
  in-flight flush can land between the two wipes (cross-table
  incoherence; shared harness for every pinning test).
  `storage/db.rs` `clear_all_asset_tables_drop` now includes
  `ASSET_BY_ALBUM`.

Fix shape for A/1: snapshot ancestor/parent album IDs before
`begin_write`, refresh inside the txn from the owned snapshot. Fix
shape for 7: drain writer+cascade queues and wait idle before wiping,
mirroring `cancel_and_wait_index_idle`.

TDD: each fix needs a test red on pre-fix code (demonstrated) and
green after. For A: deterministic runtime lock-order test at the flush
site (hold `DIR_ALBUM_CACHE`, run the real flush path in a thread,
assert `begin_write` on the main thread completes within a bound —
pre-fix the flush thread holds the txn and the test times out), plus
source-level guards in `tests/architecture.rs` (existing precedent)
for the transitor/assign sites. For 7: fenced-reset test (pending
flush submitted, reset returns, tables stay empty).

### Backlog (same review, not in this PR)

- **2. Delete path** holds read txn across mutex + file deletes
  (`router/delete.rs:340-452`): snapshot bloat + same inversion.
- **3. Write txns held across O(N) scans** — `apply_album_delta`
  (`storage/asset_store.rs:697`) and `refresh_empty_album_cover`
  (`:864`) run member listing + full `ASSET_BY_ID` scans inside the
  caller's write txn; callers include `flush_tree.rs:155-361`,
  `transitor.rs:65-139`, `assign_album.rs:423-439`. Single-writer
  stall class; candidate explanation for item 2 rebuild discrepancy
  and writer-cascade lag in `.plan/perf-investigation.md`. Fix with
  measurements, alongside the perf roadmap.
- **4. Blocking under async**: `writer_loop` runs `flush_tables` /
  `sweep_remove_batch` / `cascade_update_expire` sync on tokio workers,
  bypassing `run_cpu`/`run_io` (`tasks/queue/writer.rs:354-402,499-508`).
- **5. `InteractiveAlbum` no-op** (`tasks/queue/worker.rs:98-105`):
  parent-cover refresh on `create_dir_album` silently dropped; request
  still pays an intake round-trip. Needs a product call: restore
  refresh or remove the wait.
- **6. Orphan `ASSET_BY_ALBUM` rows**: `apply_album_delta` writes the
  member row (`storage/asset_store.rs:709`) before the album-existence
  check (`:745`); early-return leaves orphan member rows; delete/detach
  ordering can resurrect detached ids. Also `let _ = affected_album_ids`
  discards at `router/delete.rs:134`, `router/put/edit_flags.rs:162`.
- **8. Pinning tests are single-threaded end-state checks**; none pins
  the concurrency properties. Add one multi-task barrier test and one
  reset-fenced test (7 covers the latter) before treating them as
  concurrency proof.

Reviewed clean: intake `SCHED`/`LaneQueue` locks, `SUBMIT_LOCK`
ordering, `run_cpu`/`run_io` design, `path_epoch` DashMap,
`move_asset_mapping` atomicity, sweep evict-after-commit.

## Further steps: randomized concurrency detection (post-#203)

The A/1 deadlock was found only because a background album-index task
happened to run alongside the sequential stateful test —
`random_op_sequences_converge` applies ops one at a time and cannot
generate lock interleavings itself; #203's lock-order tests are
deterministic single-site probes. Goal: random exploration that
produces deadlocks/races (not just wrong end states) with the same
"every trace becomes a regression test" discipline as
`docs/quint-modeling.md`.

### Randomized tests (backend suite)

1. **Concurrent-interleaving stateful test** (extends the
   `eventual_consistency_stateful.rs` pattern): run random op
   sequences while concurrently spawning the real task mix — writer
   flushes, album-index jobs, assign/transitor requests, sweeps —
   with random yields between steps. Assert (a) completion within a
   bound (deadlock detector), (b) cross-table exactness after
   quiesce (`item_count == members.len()`, finding 6), (c) convergence.
   This is the test class that would have caught A/1 without CI.
2. **Runtime lock-order checker**: under `cfg(test)`, instrument the
   lock/txn acquisition points (`DIR_ALBUM_CACHE`, `TREE.in_disk`
   begin_write/read, later `REBUILD_LOCK`) with a thread-local
   acquisition-order registry; any acquisition that inverts the
   registered order fails immediately. Generalizes #203's source-span
   guards to helper-call sites the static scan cannot see, and runs
   inside every existing test for free.
3. **Reset fuzz** (closes finding 8's reset-fenced slot): call
   `reset_backend_state` at randomized points during active load,
   then assert clean state after settle — generalizes the
   deterministic finding-7 test.
4. **CI cost control**: keep the new proptests in the standard suite
   at default cases; add a nightly job with raised `PROPTEST_CASES`
   and/or the fuzz nightly (`utils/fuzz`) extended with the
   concurrent driver. Deadlock tests need bounded waits, not long
   timeouts, so they stay cheap when green.

### Quint / model-based testing

Current gap: `utils/quint/pipeline/eventual-consistency*.qnt` models
data state and convergence, not _resource holdings_ — deadlock
invariants do not survive that abstraction, so simulation cannot see
this bug class. Options, cheapest first:

1. **Registry REQ (cheap, do with step 2)**: register the lock-order
   convention as a `REQ-N` in `utils/quint/pipeline/requirements.json`
   with the #203 span guards + runtime checker as its live gate, so
   the invariant enters the traceability map instead of living only
   in review notes.
2. **Small resource-hold model (the substantive use)**: a bounded
   model of the two-resource protocol — actions are acquire/release
   of {cache mutex, write slot} by the three actor roles (flush,
   album-index, request); the invariant is an acyclic wait-for graph.
   The quint simulator is well suited: tiny finite state, exhaustive
   interleaving exploration, and per `docs/quint-modeling.md` every
   violating trace becomes a deterministic regression test (the
   #203 runtime test is exactly that shape). This complements
   proptest, which cannot reliably hit deadlocks.
3. **Trace replay from model to backend (later, evaluate after 2)**:
   generate op sequences from the model and replay them against the
   real backend — only worth it if the coverage tooling
   (`utils/quint/coverage`) can map the new model's properties to
   gate tests without new machinery; otherwise stay with 1+2.
