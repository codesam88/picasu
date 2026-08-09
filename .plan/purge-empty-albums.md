---
status: open
type: feature
priority: high
area: backend
---

## Context

When files are moved to `.trash/`, the directory hierarchy is mirrored (e.g.
`photos/vacation/photo.jpg` → `.trash/photos/vacation/photo.jpg`). The
`ensure_dir_albums` indexer creates album records for every intermediate
directory. When items are later restored (via `assign_album`) or permanently
deleted, those empty intermediate album directories and their DB entries remain,
showing as empty folders on the trash page.

The same can happen outside `.trash/` when moving files between albums, but
there the desired default is to keep albums (user-created). The cleanup should
therefore be gated on a frontend-supplied flag: `true` for trash-page restores,
`false` (default) for regular moves.

## Design

`purge_empty_albums(leaf_dir)` receives the deepest source directory and walks
upward. At each level it checks whether the directory is empty on disk (or
missing). If empty, the album DB record, `.albuminfo.xmp` sidecar, directory,
and cache entry are removed. The walk stops at the first non-empty directory.
This is not trash-specific — the same logic applies to any album hierarchy.

The `AssignAlbumData` request gains `cleanup_empty_source: Option<bool>`
(default `false`). The frontend sends `true` for restore operations. The backend
calls `purge_empty_albums` directly after a successful move — if the move fails
and returns early, no cleanup runs.

For permanent delete from trash (`process_deletes`), no flag is needed: the
batch sweeps empty albums unconditionally after all deletes commit.

### `move_item_into_album` flow

1. Before the transaction, read the old album's `dir_path` from DB/cache.
2. After successful commit, if `cleanup_empty_source` is `true`: call
   `purge_empty_albums(old_album_dir)`.

### `move_album_into_album` flow

1. `source_dir` is already known before the move.
2. After successful commit, if `cleanup_empty_source` is `true`: call
   `purge_empty_albums(source_dir.parent())` — the album's own directory was
   moved away, so check its former parent.

### `process_deletes` flow

1. Collect a set of leaf directories from permanently-deleted items (the parent
   of each deleted alias path).
2. After the main loop, for each leaf: call `purge_empty_albums(leaf)`
   (deduplicated, sorted deepest-first to avoid redundant walks).

## Tasks

### Backend: `dir_album.rs`

- [ ] Add `pub fn remove_dir_album_from_cache(dir_path: &Path)`:
      Lock `DIR_ALBUM_CACHE`, call `cache.remove(dir_path)`.

### Backend: `delete.rs`

- [ ] Add `pub fn purge_empty_albums(leaf_dir: &Path, data_table)`:
      Walk from `leaf_dir` upward. At each level:
  - If `current` does not exist on disk → remove album DB record + cache entry,
    continue to parent.
  - If `current` exists: remove `.albuminfo.xmp` sidecar if present, then check
    `fs::read_dir(&current).next().is_none()`. If empty → remove album DB record
    - directory + cache entry, continue to parent. If not empty → stop.
  - Album DB record removal: iterate `data_table`, find album whose
    `dir_path == current.to_string_lossy()`, remove by key. Collect removals
    first (immutable iteration), then delete.
  - Bulk-remove evicted paths from `DIR_ALBUM_CACHE` after the loop.

- [ ] In `process_deletes`, after the main `for (hash, alias_path)` loop and
      before `txn.commit()`:
  - Collect leaf dirs: for each permanently-deleted item (both
    `permanent_delete_item` and `permanent_delete_album`), insert the parent of
    the deleted alias path into a `BTreeSet<PathBuf>` (BTreeSet for automatic
    dedup and ordering).
  - After the loop, for each leaf dir (in reverse sort order, i.e. deepest
    first): call `purge_empty_albums(&leaf, &mut data_table)` using the same
    write transaction.

### Backend: `assign_album.rs`

- [ ] Add `cleanup_empty_source: Option<bool>` to `AssignAlbumData`
      (serde default `false`).

- [ ] In `move_item_into_album`:
  - Add `cleanup_empty_source: bool` parameter.
  - Before the write transaction: read old album's `dir_path` from the data
    table (open a read txn or read from the existing write txn before modifying).
  - After `txn.commit()`: if `cleanup_empty_source`:
    - Open a new write transaction, call
      `purge_empty_albums(&old_dir, &mut data_table)`, commit.

- [ ] In `move_album_into_album`:
  - Add `cleanup_empty_source: bool` parameter.
  - After `txn.commit()` (outside the existing block): if `cleanup_empty_source`:
    - Open a new write transaction, call
      `purge_empty_albums(source_dir.parent(), &mut data_table)`, commit.

- [ ] In `move_hash_into_album`: pass `cleanup_empty_source` through to both
      `move_item_into_album` and `move_album_into_album`.

- [ ] In `assign_album` endpoint: extract `data.cleanup_empty_source.unwrap_or(false)`
      and pass to `move_hash_into_album`.

- [ ] No trash-specific imports needed in `assign_album.rs`; the cleanup is
      gated purely on the `cleanup_empty_source` flag.

### Frontend: `assignAlbum.ts`

- [ ] Add `cleanupEmptySource: boolean = false` parameter to `assignAlbum()`.
- [ ] Include it in the PUT body: `{ hash, albumId, onConflict, cleanupEmptySource }`.

### Frontend: `AssignAlbumModal.vue`

- [ ] In `handleSubmit`, pass `cleanupEmptySource: isRestore.value` to each
      `assignAlbum()` call.

### Tests

- [ ] Extend `restore_via_assign_to_original_album.yaml`:
      After the restore PUT, add assertions:
  - `file_absent: /.trash/e2e_untrash_album` (directory cleaned up)
  - Optionally verify no album entry exists for the trash path (may require
    adding `album_absent` support to the scenario interpreter, or checking via
    prefetch response).

- [ ] Add new scenario `restore_cleans_empty_parent_albums.yaml`:
      Trashes a nested album (`/a/b/photo.jpg`), restores it, asserts both
      `/.trash/a/b` and `/.trash/a` directories are absent.

- [ ] Unit test for `purge_empty_albums`:
  - Empty leaf dir → album removed, dir removed.
  - Non-empty leaf dir → walk stops, nothing removed.
  - Missing directory on disk → stale album record removed.
  - Nested empty dirs → both removed in one walk.

## Notes

- `purge_empty_albums` is generic — not trash-specific. The walk stops at the
  first non-empty directory regardless of where in the hierarchy it is called.
- `compute_trash_root()` panics if `APP_CONFIG` is not initialized. This is
  fine in the request path (server is running) but tests need bootstrap.
- `rewrite_dir_album_cache_prefix` is for renaming; the new
  `remove_dir_album_from_cache` is for deletion.
