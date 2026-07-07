---
status: open
type: perf
priority: high
area: backend
---

## Notes

<!-- Agents: append progress notes below, newest first. Do not edit existing lines. -->

## Selective View Cache Invalidation

### Problem

The prefetch cache key is `hash(expression, locate, VERSION_COUNT_TIMESTAMP)`. Every mutation bumps the global version counter, which changes every cache key — every cached view is invalidated regardless of whether it was affected. A user browsing album "work" pays a full re-fetch when someone edits a tag on a photo in album "vacation".

The tree rebuild (`UpdateTreeTask`) already runs on every mutation and is the dominant cost. The unnecessary cache misses compound this: every re-fetch re-filters the full tree and writes a new `TREE_SNAPSHOT`, while the unaffected views could have served their old snapshot.

### Key aspects

**1. Cache key vs cache entry lifecycle.** The version counter lives in the key hash. Bumping it atomically invalidates everything because no lookup matches the old hash. To keep some entries alive, the version must leave the key — but redb table names are versioned (free cleanup via `ExpireCheckTask`), creating tension between stable keys and versioned storage.

**2. Indexing what a query references.** A cached query may filter by album (`album:vacation`), tag (`tag:beach`), or text search (`Any("sunset")`). A mutation affects specific albums (moving an item), tags (adding/removing), or textual metadata (description change). To invalidate only matching entries, we need a reverse index: `mutation dimensions → which cached entries intersect`.

**3. Coalescing burst mutations.** Multiple rapid mutations (batch tag edit on 100 photos, indexer ingesting a directory) should not cause 100 cache invalidations. A debounce or rate-limiter that collapses a burst into a single invalidation event is the right tool, but the optimal window differs by mutation type.

### Approaches considered

**A — Per-dimension version counters**
Each album and tag gets its own atomic; the cache key hashes the relevant subset of counters for the expression. A mutation bumps only the counters of affected albums/tags, so only queries referencing those dimensions miss.

- Pro: Fully precise — unaffected queries never miss.
- Con: Expression processing at key-build time must query N atomics; tag expansions from `Any()` are ambiguous; adds complexity to every cache lookup.
- Con: No clean answer for textsearch (which covers multiple dimensions).

**B — Full selective eviction (all dimensions, all mutations)**
Remove `VERSION_COUNT_TIMESTAMP` from the cache key entirely. Maintain an in-memory index (`query_hash → {album_ids, tags, is_textsearch}`). Every mutation type does targeted eviction via the index: walk matching entries, delete from DashMap + redb.

- Pro: Most correct — no unnecessary cache misses for any mutation type.
- Con: Requires per-endpoint logic for all mutation handlers to compute affected dimensions and submit invalidation. Touches every mutation endpoint.
- Con: Redb persistence of the index needed for restart correctness; without it, index is lost on restart and entries become "leaked" (never selectively invalidated, only expired by version rotation).

**C — Two-tier: selective album + rate-limited global**
Split mutation types. Album-scoped mutations (`assign_album`, `set_album_cover`, etc.) use selective eviction. All other mutations (`edit_tag`, `edit_flags`, etc.) use the existing global version counter, rate-limited to a 10s cooldown. Cache key removes the version, enabling selective survival.

- Pro: Album mutations (common in album-browsing workflows) are genuinely cheap — unaffected views survive.
- Pro: Rate-limiter prevents thrashing on bulk operations; the global fallback keeps other mutation endpoints unchanged from current code.
- Pro: No redb persistence needed for the index — the in-memory index is rebuilt as queries re-populate after restart.
- Con: Non-album mutations still invalidate everything (just rate-limited).
- Con: Tag-based and textsearch-based selectivity is deferred to v2.

### Selected: Approach C (two-tier)

**How it works:**

1. **Cache key becomes stable:** `hash(expression, locate)` — version removed. Redb tables stay versioned for free expire cleanup.
2. **`QUERY_META`: `DashMap<u64, CacheMeta>`** in-memory index tracking per-entry `{album_ids, tags, is_textsearch}`.
3. **Album mutations:** scan `QUERY_META` via `collect_matching_hashes()`, remove matches from `QUERY_SNAPSHOT` DashMap + `QUERY_META`, then bump `VERSION_COUNT_TIMESTAMP`. Unaffected entries survive in DashMap and serve from the stable key.
4. **Other mutations:** submit to `VersionRateLimiter` (10s cooldown). On fire: clear ALL `QUERY_SNAPSHOT` DashMap entries + `QUERY_META`, bump version. Every subsequent prefetch misses → fresh data.

**Race guard:** selective eviction removes from DashMap before bumping the version, so no stale DashMap hit leaks through.

**Pros:** Minimal change to existing redb architecture; album mutations are cheap; rate-limiter prevents thrashing.

**Cons:** Non-album mutations still invalidate everything; `QUERY_META` is in-memory only (lost on restart — entries from prior session are not selectively invalidatable until they're re-queried and re-indexed).

**Corner cases:**
- *Restart:* `QUERY_META` empty. Redb entries from before restart have no index entry. They survive until the next rate-limited global clear or album mutation (which bumps the version, changing the redb table name). A brief window where old entries serve stale data but are not selectively evictable.
- *Album mutation during rate-limiter cooldown:* album entry removed from DashMap. Rate-limiter later clears remaining entries. No double-clear issue.
- *Multiple album mutations in sequence:* each performs selective eviction + version bump. Multiple redb tables accumulate, cleaned by `ExpireCheckTask`. No correctness issue.
- *FlushQuerySnapshotTask + stale version:* async flush writes DashMap entries to the redb table named by whichever `VERSION_COUNT_TIMESTAMP` is current at flush time. After a version bump, flush writes to the new table. Correct.

### Implementation sketch

**New:**
- `backend/src/storage/rate_limiter.rs` — `VersionRateLimiter` with 10s cooldown, background task clears both DashMaps + bumps atomic
- Backend global init of the rate-limiter in `lib.rs`

**Modified:**
- `backend/src/storage/cache.rs` — `CacheMeta` struct, `QUERY_META` DashMap, `collect_matching_hashes()`, `remove_query_entry()`, `clear_all()`
- `backend/src/model/expression.rs` — `Expression::cache_meta()` walks tree, extracts album_ids/tags/is_textsearch
- `backend/src/router/get/get_prefetch.rs` — remove version from `build_cache_key`, store `CacheMeta` on cache-write
- `backend/src/router/put/assign_album.rs` — replace `VERSION_COUNT_TIMESTAMP.fetch_add(1)` with selective invalidation + bump; surface old_album from move helpers
- `backend/src/router/put/edit_tag.rs` — submit rate-limiter request after tree update
- `backend/src/router/put/edit_flags.rs` — same
- `backend/src/router/put/edit_rating.rs` — same
- `backend/src/router/put/edit_description.rs` — same
- `backend/src/router/delete.rs` — same
- `backend/src/router/put/edit_album.rs` — selective invalidation after cover/title update
- `backend/src/router/put/edit_share.rs` — selective invalidation after share update
- `backend/src/tasks/actor/index.rs` / `backend/src/tasks/batcher/flush_tree.rs` — rate-limiter for indexer batches
- `backend/src/storage/mod.rs` — add `pub mod rate_limiter`
- frontend: `AssignAlbumModal.vue`, `EditBatchTagsModal.vue` — replace `refreshGalleryAfterMutation` with `albumStore.fetchAlbums()`

### Open items (resolved)

**1. Indexer integration** — The filesystem watcher dispatches `index_image` per-file via `INDEX_RUNTIME` (1s debounce per path, no batch aggregation). Each file triggers `FlushTreeTask` → `UpdateTreeTask`. Resolution: use `std::sync::mpsc` (not tokio) so `request_bump()` is thread-safe across `INDEX_RUNTIME` and `ROCKET_RUNTIME`. The background thread uses `recv_timeout` for the 10s cooldown. Indexer coalesces naturally: 50 files in 10s → one invalidation.

**2. `delete_share`** — Album metadata mutation (same handler as `edit_share.rs`). Resolution: album-selective invalidation, same bucket as `set_album_cover`, `set_album_title`. Not rate-limited.

**3. Restart + cold QUERY_META** — Moot. On startup, `UpdateTreeTask` runs → `UpdateExpireTask` swaps `VERSION_COUNT_TIMESTAMP` to a fresh timestamp, changing the redb table name. Previous-session entries are in the old table → unreachable regardless of QUERY_META. Cache is fully cold. QUERY_META populated as queries re-fill. No action needed.
