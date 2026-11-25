---
status: backlog
type: perf
priority: medium
area: backend
---

## View cache eviction strategy

**Status:** deferred pending the lazy-fetch option verdict (see
`perf-investigation.md` item 9). The current hybrid — memory budget plus redb
spill — stays as-is on this branch and is not to be merged until that verdict
is recorded. The eviction code itself (`ebc51874`) exists only on its
exploration branch; this record does not ship it.

### Current state

`temp_db.redb` (backed by `TREE_SNAPSHOT`) caches materialized filtered/sorted
views of the gallery. On every prefetch cache miss, `get_prefetch.rs` computes
`Vec<ReducedData>` for the filter and inserts it into
`DashMap<i64, Vec<ReducedData>>` keyed by the snapshot id.

The map is budget-bounded: the resident row buffers are capped by
`PICASU_TREE_SNAPSHOT_MEMORY_MB` (default 64 MiB; `0` disables retention), and
`FlushTreeSnapshotTask` evicts the oldest ids until the resident set fits,
writing each to its own per-timestamp redb table before dropping the memory
copy. Oldest-id order is creation order, i.e. the view least likely to be read
again, not access order. `read_tree_snapshot` serves memory first and falls
back to disk, so an evicted id still resolves.
`remove_tree_snapshot::remove_task` drops the memory copy and then the table
under one lock, so an in-flight eviction cannot resurrect a table the expire
pass has already deleted.

`ExpireCheckTask` (triggered by `update_expire_task` on data changes, plus a
24h looper cycle) deletes stale tables from both `temp_db.redb` and
`cache_db.redb` based on expiry dates in `expire_db.redb`.

### Measured (2026-11-17, release, 5653-row rig, 200 rounds of one miss each)

A distinct `locate` value keeps the tree version valid and forces the miss.

| variant                 | tree tables | `temp_db.redb` | settled scroll bar p50 | prefetch p50 | VmHWM  |
| ----------------------- | ----------- | -------------- | ---------------------- | ------------ | ------ |
| pre-budget              | 201         | 269 MB         | 5.26 ms                | 66.1 ms      | 263 MB |
| budget 64 MiB (default) | 131         | 135 MB         | 2.20 ms                | 66.5 ms      | 259 MB |
| budget 512 MiB          | 0           | 0 MB           | 2.20 ms                | 69.1 ms      | 249 MB |

- "Settled" = the read a client issues after the prefetch has settled; the
  pre-budget build has already flushed and dropped the snapshot by then, so
  the read decodes the whole table from redb.
- Immediate reads are unchanged: the pre-budget flush is detached and usually
  still pending when the first read lands.
- Process peak does not drop with the default budget because redb's page cache
  (1 GiB default) roughly replaces the retained rows. The knob moves the
  snapshot store between RAM and disk one-to-one; it does not shrink the
  store, only bounds it.
- Micro-measured (release, rows in parentheses): scroll bar read 0.105 ms
  resident vs 1.744 ms from disk (5437), 6.67 vs 64.8 ms (200k); row
  batch 0.0021 vs 0.0123 ms (5437); eviction write 12.8 ms (5437), 497 ms
  (200k).
- The evicted-id fallback is verified end-to-end: the first snapshot of the
  run is far outside any budget by the last round, and its scroll bar read
  returns 200 from disk.

### Proposed work

1. **Benchmark current behavior** — partially done (see the table above):
   measure RAM and disk at 1M-item scale, cache-cold refilter latency, and
   flush throughput.
2. **Replace oldest-id eviction with LRU** — evict the _least recently used_
   snapshot instead of the oldest id. The `get_scrollbar`/`get_row` endpoints
   already look up by id and would have to record the access.
3. **Configurable in-memory limit** — done: `PICASU_TREE_SNAPSHOT_MEMORY_MB`,
   env-only so it needs no config schema or API surface. The flush is now
   budget-triggered rather than triggered per request.
4. **Re-evaluate the disk spillover model** — with a budget + oldest-id
   eviction, is the redb backing worth it, or should a snapshot that falls out
   of memory be re-computed from `TREE.in_memory` on the next miss? Retention,
   not eviction, is what keeps `temp_db.redb` large: a table is deleted only
   after the version it belongs to expires (~1 h), while the prefetch token
   that reads it lives 300 s. See `perf-investigation.md` item 9.

### Related tasks on the board

- `prefetch-snapshot-expiry` (DONE, bug, high) — the missing expire-table row
  used to read as "expired", so a fresh snapshot could vanish; `remove_task`
  now also has to purge the memory copy, because a snapshot that never
  overflowed the budget exists only there.
