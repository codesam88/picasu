---
status: done
type: feature
priority: high
area: backend
---

## Notes

Unify filesystem rebuild into `index_album`: retire `rebuild_from_filesystem`

- `POST /post/rebuild`, replace with drop-tables + `POST /post/index/album`
  with full coverage.

Problem: two walk paths. `rebuild` bypasses `DIR_ALBUM_CACHE` (stale
`path -> old_id`, missing `new_id`, album filter empty until restart),
skips job/status/cancel, and keeps orphan identity rows for unprocessable
files (implicit `None` payload → silent `0x0` tiles). `scan` drops failed
files entirely (invisible despite bytes on disk) and skips empty dirs.

Goal: one async indexed path that (1) registers empty albums, (2) uses
`delete_table` so schema-bumped `METADATA_TABLE` rows are clearable,
(3) stores explicit error stubs (identity + `pending`/error payload) so
broken files render as placeholder + download/delete with tooltip, and
surfaces `failed_paths` in `AlbumIndexStatus`.

## Implementation steps

1. Empty albums in `tasks/actor/album_index.rs`: dir pass calling
   `get_or_create_dir_album` per dir (parent-first, skip internal roots);
   extend `sweep_stale_asset_paths` to `evict_dir_album` + remove dead
   album records. Pin: empty dir indexed → album exists; deleted dir →
   evicted.
2. Table-drop helper (`storage/db.rs` or `process/rebuild.rs` replacement):
   `delete_table` for `ASSET_BY_PATH/BY_ID/DUPE_INDEX/METADATA_TABLE`
   instead of row iteration; wire as `clear:true` option or separate reset
   before `index_album("/")`. Pin: pre-bump fixture clears without panic.
3. Explicit error stubs: `IndexTask` failure → insert identity +
   error-marked payload (not drop); `UpdateTreeTask`/`compose_abstract_data`
   preserves it; status carries `failed_paths`. Pin: truncated JPEG →
   placeholder row + failed entry, album-filterable, downloadable.
4. Retire `process/rebuild.rs` + `router/post/rebuild.rs`: update OpenAPI,
   `ser_de.rs` panic text, `docs/database.md`, scenario
   `rebuild_populates_asset_tables.yaml` → drop+scan flow.
5. Regression: share/album prefetch after drop+scan without restart returns
   N rows (covers `bug-rebuild-dir-album-cache`); full backend + scenario
   suite green.
