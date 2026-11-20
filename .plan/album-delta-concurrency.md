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

### Status (2026-11-21, branch `fix/album-concurrency`)

Landed: R2 checker (`d1a6a34d`), A/1 snapshots (`15c71a25`), fenced
reset (`e3b386c5`), 1b remove-path snapshot + barrier (`c69c11fd`), 6
check-before-write + no-orphan tests (`467910e3`), 2 delete read-txn
release (`d960e1be`), intake fence + mid-flight reset test (`042706e1`),
concurrent harness (`61e1a6a7`), harness move axis (`fb3a1eaa`),
aggregate proptests (`85c86027`), intake cancel + parked test + REQ-17
(`56dc5893`), ignored txn-hold probe (`d089ee7e`). Every fix TDD
red/green with bounded waits; `just backend-test` green.
Rejected: split-txn narrowing refactor (separate read/write txns for
member row vs aggregate — breaks I2 same-txn atomicity for ~15% gain;
see item 3). Lazy-split design decided (next section); XMP cover
attribute round-trip dispatched as first worker item.

Full harness: `concurrent_op_sequences_converge` (30 cases, writer-flush

- dir-album + store-level move mix, 30 s detector, exactness after
  quiesce). Mix does not yet include assign/transitor requests or sweeps
  — same wrapper pattern extends.

### Album aggregates lazy split (decided 2026-11-21)

Probe (`measure_txn_hold_scaling`, ignored by default, N=1000):
`descendant_cover` 143 ms, `apply_album_delta` recompute ~400 ms,
linear; write-hold ~= read time (scan CPU dominates). Split-txn
refactor rejected: separate txns for member row vs aggregate break I2
same-txn atomicity for ~15% gain. The O(N²) term is per-asset ancestor
`descendant_cover` fan-out in flush, not lock type.

1. Same-txn `apply_album_delta` keeps O(1) member row + `item_count` /
   `item_size` + dirty-mark. Always-exact for count/size/member (grid
   counts, filters). I1/I2 hold for these fields.
2. Range/cover go lazy: recompute-on-read-if-dirty. FS events mark dirty
   via the same delta path; the read path recomputes and writes back in
   a short txn. I1/I4 and harness exactness scope to quiescence for
   range/cover only.
3. Cover rule changes newest → oldest direct member (range scan already
   yields min-ts; pins at `asset_store.rs:1507` updated). Non-recursive
   fallback: no direct members → alphanumeric first sub-album cover
   (timestamp order would need lazy ranges first — circular).
   Manual cover exists (`PUT /put/set_album_cover` + `ItemSetAsCover`,
   DB-only, no kind/membership check) with no manual-vs-auto flag —
   add the marker; lazy pass validates manual first (live member → keep;
   removed/trashed → oldest-pick fallback + clear marker). Unconditional
   recompute on update/remove (`asset_store.rs:800-850`) and
   newer-wins on add (`:822-830`) currently clobber manual covers.
4. Bulk moves use batch-form deltas grouped by album (one member-set +
   one count/size patch + one dirty-mark per affected album); single
   asset = one-element batch. Same function, not two. — REJECTED
   2026-11-21: batching across per-asset txns splits the asset write
   from its delta (torn rows on crash between commits, no healer covers
   it, wider blast radius). Per-asset O(1) same-txn deltas retained;
   the N² term is already gone via lazy refresh (slice C).
5. Sidecar stays XMP: `.albuminfo.xmp` is already read (`dir_album.rs:
262-315`) and written (`sidecar_edit` → `xmp_write`, all edit
   endpoints) for title/desc/tags/rating/trash; only the cover was
   DB-only (`edit_album.rs:110-112`). Add the cover attribute to the
   picasu XMP namespace instead of a new TOML format (no directory-XMP
   interop exists — digiKam keeps album metadata DB-only, Picasa used
   per-folder `.picasa.ini` — but XMP keeps one parser and the existing
   read/write/ignore paths). `set_album_cover` persists via sidecar;
   rebuilds keep manual covers.

Slices: A) contract + dirty flag + manual marker + invariant scoping;
B) strip range/cover from `apply_album_delta`, mark dirty; C) read-
if-dirty refresh (validate-then-oldest + alphanumeric fallback); D)
batch-form deltas for bulk paths (flush batch, detach, directory
moves).

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
- **3. Write txns held across O(N) scans** — SUPERSEDED by the lazy
  split above: per-asset scans stay in-txn only for the O(1) member/
  count/size path; range/cover move to read-if-dirty; bulk fan-out
  fixed by batch-form deltas (slice D). Probe kept as
  `measure_txn_hold_scaling` (ignored). Original sites:
  `apply_album_delta` (`storage/asset_store.rs:697`) and
  `refresh_empty_album_cover` (`:864`); callers `flush_tree.rs:155-361`,
  `transitor.rs:65-139`, `assign_album.rs:423-439`.
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

1. **Registry REQ (landed as REQ-17 in `56dc5893`)**: lock-order
   convention registered with the #203 span guards + runtime checker
   as its live gate.
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
