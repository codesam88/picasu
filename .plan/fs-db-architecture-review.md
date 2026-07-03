---
status: backlog
type: chore
priority: medium
area: backend
---

## FS/DB architecture review — perf & robustness, O(1) album moves

Goal: review how the backend manages filesystem and database structures,
anchored in a semi-formal architecture description, then use it to drive a
scoped optimization/cleanup pass. Two objectives: performance and
robustness.

### Motivating example

Moving a top-level album containing several sub-albums and images (in
subalbums) via "Assign Album" should be an O(1) operation — it should not
require rewriting metadata for every contained image/subalbum. The current
implementation (`move_album_into_album` in
`backend/src/router/put/assign_album.rs`, added while fixing sub-album
moves) does a full `DATA_TABLE` scan + per-record path-prefix rewrite for
every descendant image/video/nested-album, plus an in-memory
`DIR_ALBUM_CACHE` prefix rewrite (`rewrite_dir_album_cache_prefix` in
`dir_album.rs`) — both O(descendants). This was a deliberate stopgap (the
album-move feature didn't exist at all before this fix), not the intended
long-term design.

### Steps

1. **Overview**: produce an architecture overview anchored in a
   semi-formal description — an entity-relationship diagram (e.g. Mermaid
   `erDiagram`) covering every persisted/in-memory entity (`AbstractData`
   variants, `ObjectSchema`, `ImageMetadata`, `VideoMetadata`,
   `AlbumMetadata`, `FileModify`, `Share`, `DIR_ALBUM_CACHE`,
   `TREE.in_memory`, the cache DBs, etc.) with documented relationships,
   plus prose explaining each entity's purpose. Add as a new chapter in
   `docs/design.md`.
2. **Catalog operations**: list every lookup (read) and modification
   (write) API — file:line, which DB/cache table(s) or in-memory
   structure(s) it touches, and how it affects the entities from step 1.
3. **Identify critical paths**: from the catalog, flag
   performance-critical paths (hot per-request paths, and any
   O(n)/O(descendants) operation — the album-move cascade above is the
   known example) and robustness challenges (partial-failure/crash
   consistency between DB and filesystem — see
   `fs-db-transaction-journal.md`; stale-cache scenarios — see
   `stale-dir-album-cache.md`; watcher/DB path-identity assumptions).
4. **Benchmarks and tests**: establish benchmarks and regression tests for
   the identified critical paths _before_ changing anything, so
   improvements are measurable and regressions are caught.
5. **Clean up / optimize**: only then design and implement the
   optimization pass. The specific target: O(1) album moves via
   parent-pointer/ID-based membership instead of absolute-path storage and
   rewriting.

### Prior investigation (2026-07-02)

Two research passes while fixing the sub-album-move bug found:

- No hidden semantic dependency blocks a parent-pointer redesign: every
  read of `alias[].file` / `AlbumMetadata.dir_path` as an absolute path
  string is either disk I/O (open/rename/read/write) or a path-comparison
  used purely for membership/parent lookup — both are compatible with
  resolving the path on demand instead of storing it.
- `DIR_ALBUM_CACHE` (`dir_album.rs`) already implements half of a
  parent-pointer model: `get_parent_album_id` derives "parent album" by
  looking up `dir_path.parent()` in a path-keyed map, rather than storing
  the relationship as a first-class field. `ImageMetadata.album` /
  `VideoMetadata.album` already exist and could become the authoritative
  parent-pointer field for media items if membership checks were switched
  from path-comparison (`Path::new(&item.alias.file).parent() ==
dir_path`) to ID-comparison (`item.album == album_id`) — at which point
  a directory move would only need to update the _moved_ album's own
  parent pointer, since descendants' immediate-parent relationship never
  changes regardless of nesting depth.
- The one genuinely tricky spot: `handle_removed_file` in
  `start_watcher.rs` matches an incoming absolute OS path from a
  `notify::Remove` event against `alias[].file` by exact string equality
  — this needs `alias.file` (or an equivalent on-demand resolution:
  parent album's resolved directory + filename) to work, and would need
  reworking from a full-table linear scan into a bounded parent-lookup +
  filename match either way.
- Roughly 17-18 files read these two fields as raw absolute-path strings
  today (`model/abstract_data.rs`, `model/album.rs`,
  `process/xmp_write.rs`, `process/xmp.rs`, `process/dir_album.rs`,
  `router/get/get_img.rs`, `router/get/get_list.rs`, `router/delete.rs`,
  `router/put/assign_album.rs`, `router/put/edit_album.rs`,
  `router/post/create_dir_album.rs`, `tasks/actor/deduplicate.rs`,
  `tasks/actor/index.rs`, `tasks/batcher/start_watcher.rs`,
  `tasks/batcher/flush_tree.rs`, `storage/ser_de.rs`) — most are one-shot
  I/O or single-hop parent lookups, not cascades. The two genuinely
  removable O(descendants) costs are `rewrite_paths_under` /
  `move_album_into_album` in `assign_album.rs` and
  `rewrite_dir_album_cache_prefix` in `dir_album.rs`, both added as part
  of the sub-album-move fix.

Full agent transcripts are not preserved — re-derive via fresh code search
when this task is picked up, since the codebase will have moved on.

### Related

- `fs-db-transaction-journal.md` — DB/filesystem crash-consistency,
  overlaps with step 3's robustness angle.
- `stale-dir-album-cache.md` (done) — prior related fix to
  `DIR_ALBUM_CACHE`/`assign_album` consistency.

## Notes

- 2026-07-02: Filed after fixing assign_album's sub-album-move bug (it
  previously 400'd for any Album-type item; fixed by adding a full
  recursive path-rewrite as a stopgap). User asked to review the whole
  FS/DB management approach for perf + robustness before optimizing
  further, rather than picking a shortcut immediately. Tabled for a
  dedicated future session — not started.
