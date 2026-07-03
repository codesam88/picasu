---
status: backlog
type: perf
priority: medium
area: backend
---

## View cache eviction strategy

### Current state

`temp_db.redb` (backed by `TREE_SNAPSHOT`) caches materialized filtered/sorted
views of the gallery. On every prefetch cache miss, `get_prefetch.rs` computes
`Vec<ReducedData>` for the filter, inserts into `DashMap<i64, Vec<ReducedData>>`
keyed by `Utc::now()`, then immediately triggers `FlushTreeSnapshotTask` which:

1. Takes the next entry from the DashMap (random shard order — not LRU)
2. Writes it entry-by-entry to a per-timestamp redb table
3. Removes it from DashMap

The DashMap is therefore an _ephemeral write buffer_ whose entries are drained
on whatever request happens to be the next cache miss — not a long-lived cache.
There is no eviction policy, no memory limit, and no LRU tracking. The redb
backing makes evicted entries still addressable by timestamp, but at the cost
of serializing/deserializing every entry through bitcode.

Separately, `ExpireCheckTask` (triggered by `update_expire_task` on data
changes, plus a 24h looper cycle) deletes stale tables from both
`temp_db.redb` and `cache_db.redb` based on expiry dates in `expire_db.redb`.

### Proposed work

1. **Benchmark current behavior** — establish baseline:
   - Measure RAM used by DashMap at various cache miss rates (single filter,
     N distinct filters) and gallery sizes (10K, 100K, 1M items).
   - Measure redb disk usage for the same scenarios.
   - Measure request latency with cache hot (DashMap), cache warm (redb),
     cache cold (re-filter from `TREE.in_memory`).
   - Measure flush task throughput (entries/ms).

2. **Replace FIFO eviction with LRU** — the flush task should evict the
   _least recently used_ snapshot, not the first one in DashMap iteration
   order. Options:
   - Wrap DashMap with an access-order tracking structure (e.g.
     `LinkedHashMap`- or `lru` crate-backed).
   - Or maintain a `BTreeSet<(last_access_time, timestamp)>` side-index.
   - The `get_scrollbar`/`get_row` endpoints (which already look up by
     timestamp) should also update the access time.

3. **Configurable in-memory limit** — add a setting (e.g.
   `snapshot_cache_memory_mb` in `config.json`) that caps how many
   `Vec<ReducedData>` entries the DashMap holds before evicting to disk
   (or refusing to cache, if no disk backing). Below the limit, keep
   everything in memory and skip the redb flush entirely.

4. **Re-evaluate the disk spillover model** — with LRU + in-memory limit,
   do we need the redb backing at all, or can snapshots that fall out of
   the LRU cache simply be re-computed from `TREE.in_memory` on the next
   miss? Questions to answer:
   - How often does the same filter+scroll position get requested twice?
   - If re-computation is fast enough (filter is O(N) scan of
     `DatabaseTimestamp` + timestamp resolution on `AbstractData`), is
     the redb spillover worth the complexity of 3 ephemeral DBs + 2 flush
     tasks + 1 expire task + 1 expire loop?
   - If we keep disk spillover, should flushes be size-based (trigger when
     DashMap exceeds threshold) rather than triggered per-request?

### Related tasks on the board

- `prefetch-snapshot-expiry` (OPEN, bug, high) — the expiry logic has a
  correctness bug that interacts with this redesign.
