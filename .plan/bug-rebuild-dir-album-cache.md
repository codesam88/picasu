---
status: done
type: bug
priority: high
area: backend
---

## Problem

`POST /post/rebuild` re-registers albums via the asset tables but never
repopulates `DIR_ALBUM_CACHE` (only `init_dir_album_cache()` at startup,
`ensure_dir_albums`, and `create_dir_album` touch it). After a rebuild,
album-scoped filtering (`Expression::Album`, used by every share's
snapshot rewrite) matches nothing: a share guest sees an empty album
until the server restarts. Found by the Schemathesis pilot (F0):
share-alpha prefetch returned `dataLength 0` post-rebuild with 10
assets present; identical call after restart returned `10`.

## Goal

Rebuild leaves the dir-album cache consistent: either repopulate it
from the rebuilt tables, or invalidate it so lookups fall through to
the database.

## Steps

1. Reproduce over HTTP: seed, rebuild, share-prefetch shows 0 rows;
   restart, shows N. Pin as a regression test.
2. Fix in `rebuild_from_filesystem` (or its caller): refresh or clear
   `DIR_ALBUM_CACHE` once the tables are rebuilt.
3. Re-run reproduction + full backend suite; confirm no restart needed.

## Outcome

Superseded by `unify-index-rebuild` (done, 2026-10-24): the defective
endpoint was retired rather than patched — `POST /post/rebuild` no
longer exists, replaced by `POST /post/index/album` with `clear`. That
path maintains the cache three ways: `clear_asset_tables_and_caches`
resets it, `get_or_create_dir_album` re-registers every directory
parent-first, and `sweep_stale_asset_paths` evicts deleted ones.

Regression pinned by
`tasks::actor::album_index::tests::bug_rebuild_dir_album_cache_reset_scan_returns_rows`
— an album-filtered prefetch returns N rows after reset+scan with no
restart (verified passing 2026-10-27, alongside the index/evict pins).
The Schemathesis harness seed dropped its restart workaround for the
same reason.
