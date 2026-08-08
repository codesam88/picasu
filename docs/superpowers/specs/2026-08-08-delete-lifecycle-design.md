# Delete Lifecycle Design — v0.1

## Overview

Delete is two-stage: **first delete** moves to a configurable trash directory
(filesystem rename), **second delete** from trash permanently removes files and
DB records. **Untrash** moves files back from trash to their original location.
The trash directory is a normal directory album — no `is_trashed` flag, no
special metadata. Trash state is derived from filesystem location.

Multi-alias photos appear in all their locations simultaneously. A photo with
one alias in `.trash/` and one in `album/` shows on both the trash page and the
album page.

## Configuration

Two new fields in `AppConfig` / `TomlGallery`:

| Field             | Type     | Default    | Description                                                                           |
| ----------------- | -------- | ---------- | ------------------------------------------------------------------------------------- |
| `trash_enabled`   | `bool`   | `true`     | When true, first delete moves to trash. When false, first delete permanently removes. |
| `trash_directory` | `String` | `".trash"` | Subdirectory name under `IMAGE_HOME`.                                                 |

Follows the existing config pattern: `AppConfig`, `TomlGallery`, both `From`
impls, `PartialUpdateConfigRequest`, `ConfigResponse`, handlers, test helpers.

## Delete Endpoint: `DELETE /delete/delete-data`

### Request

```rust
pub struct DeleteItem {
    pub index: usize,
    pub alias_path: String,
}

pub struct DeleteList {
    pub delete_list: Vec<DeleteItem>,
    pub timestamp: i64,
}
```

Every delete must specify which alias to act on. No ambiguity, no fallback.

### Logic per `DeleteItem`

1. Resolve `index` → `AbstractData` via tree snapshot
2. Find alias where `file == alias_path`; warn + skip if not found
3. Read `trash_enabled` and `trash_directory` from `APP_CONFIG`
4. Compute trash root: `IMAGE_HOME / trash_directory`

**If NOT in trash AND `trash_enabled`:** (→ move to trash)

- **Album:** `fs::rename(dir_path, trash_root/dir_name)`. Update album
  `dir_path`. `rewrite_paths_under()` for all nested records.
  `rewrite_dir_album_cache_prefix()`. Mark old parent + trash parent for
  `AlbumSelfUpdateTask`.
- **Image/Video:** `fs::rename(alias.file, trash_root/relative_path)`. Update
  alias entry in DB.

**If in trash OR `trash_enabled == false`:** (→ permanent delete)

- **Album:** `fs::remove_dir_all(dir_path)`. Remove album DB record. For each
  member: delete all aliases' files + sidecars + thumbnails, remove DB record.
  Recurse into child albums. Evict from `DIR_ALBUM_CACHE`.
- **Image/Video:** `fs::remove_file(alias.file)`. Remove sidecar. Remove alias
  from record. If alias list empty: delete thumbnail, remove DB record. Else:
  flush updated record.

## Untrash Endpoint: `PUT /put/restore-from-trash`

Structurally similar to `assign_album` — moves files and updates metadata.

### Request

```rust
pub struct RestoreItem {
    pub index: usize,
    pub alias_path: String,
}

pub struct RestoreList {
    pub restore_list: Vec<RestoreItem>,
    pub timestamp: i64,
}
```

### Logic per `RestoreItem`

1. Resolve `index` → `AbstractData`
2. Find alias where `file == alias_path`
3. Verify `alias_path` starts with trash root; reject if not (400)
4. Compute original path: strip trash root prefix from `alias_path`

**If original location is free:**

- **Album:** `fs::rename(trash_path, original_path)`. Update `dir_path`.
  `rewrite_paths_under()`. `rewrite_dir_album_cache_prefix()`. Mark albums for
  update.
- **Image/Video:** `fs::rename(trash_alias.file, original_path)`. Update alias
  entry.

**If original location is occupied (conflict):**

- `on_conflict` parameter: `skip` (default) / `rename` / `replace`
- Same conflict logic as `assign_album`

**Multi-alias:** Only the targeted alias is restored. Other aliases (trashed or
not) are unchanged.

## Filter System Changes

**Remove:** `Expression::Trashed` from `expression.rs`, the `trashed` lexer
token, and related visitors.

**Page filters:**

| Page           | Before          | After               |
| -------------- | --------------- | ------------------- |
| Timeline       | `trashed:false` | `not(album:.trash)` |
| Favorites      | `trashed:false` | `not(album:.trash)` |
| Videos         | `trashed:false` | `not(album:.trash)` |
| Archived       | `trashed:false` | `not(album:.trash)` |
| Albums         | `trashed:false` | `not(album:.trash)` |
| Album contents | `trashed:false` | `not(album:.trash)` |
| Trash          | `trashed:true`  | `album:.trash`      |

The `.trash` album ID is resolved from the directory path via `DIR_ALBUM_CACHE`.

## Watcher

Ignore events under the trash directory prefix.

## Data Model

- **Remove** `is_trashed: bool` from `ObjectSchema`
- **Remove** `set_trashed()` from `AbstractData`

## Frontend Changes

| File                        | Change                                                                                               |
| --------------------------- | ---------------------------------------------------------------------------------------------------- |
| `ItemPermanentlyDelete.vue` | Resolve `aliasPath` per item. Send `(index, aliasPath)` pairs. Call `refreshGalleryAfterMutation()`. |
| `ItemDelete.vue`            | Same — first delete (trash move).                                                                    |
| `ItemRestore.vue`           | Resolve `aliasPath`. Call `PUT /put/restore-from-trash` with `(index, aliasPath)` pairs.             |
| Page `basicString` filters  | Update each page component.                                                                          |
| `TrashedPage.vue`           | Filter: `album:.trash`.                                                                              |

### Alias Path Resolution

```typescript
function resolveAliasPath(item: EnrichedUnifiedData): string {
  if (route.meta.baseName === "album" && albumDir) {
    const match = item.alias.find((a) => a.file.startsWith(albumDir));
    if (match) return match.file;
  }
  return item.alias[0].file;
}
```

## Test Cases

### API E2E Scenarios (12 scenarios)

| #   | File                                           | Covers                                                                               |
| --- | ---------------------------------------------- | ------------------------------------------------------------------------------------ |
| 1   | `trash_and_permanent_delete_image.yaml`        | trash photo → verify in .trash/ → permanent delete → verify gone                     |
| 2   | `trash_and_permanent_delete_album.yaml`        | trash album → verify cascade → permanent delete album → verify cascade               |
| 3   | `trash_multi_alias.yaml`                       | photo with 2 aliases → trash one → verify moved, other in place                      |
| 4   | `permanent_delete_alias_preserves_record.yaml` | photo with trash alias + live alias → permanent delete trash alias → record survives |
| 5   | `trash_disabled_permanent_delete.yaml`         | `trash_enabled: false` → delete → immediate permanent delete                         |
| 6   | `custom_trash_directory.yaml`                  | `trash_directory: ".picasu_trash"` → delete → verify in custom dir                   |
| 7   | `trash_read_only_mode_blocked.yaml`            | `read_only_mode: true` → delete → 403                                                |
| 8   | `trash_edge_cases.yaml`                        | nonexistent alias warns+skips, idempotent second delete, cache eviction              |
| 9   | `untrash_image_and_album.yaml`                 | untrash single image → verify restored, untrash album → verify restored              |
| 10  | `untrash_conflict_handling.yaml`               | untrash to occupied location: skip, rename                                           |
| 11  | `untrash_not_in_trash_rejected.yaml`           | attempt untrash on non-trashed item → 400                                            |
| 12  | `watcher_ignores_trash_events.yaml`            | move file to .trash/, verify no DB update from watcher                               |

### Playwright UI Scenarios (6 scenarios)

| #   | File                                | Covers                                                      |
| --- | ----------------------------------- | ----------------------------------------------------------- |
| 1   | `delete-to-trash-flow.yaml`         | delete photo + delete album → both visible in trash         |
| 2   | `trash-permanently-delete.yaml`     | permanently delete photo + album from trash                 |
| 3   | `trash-restore.yaml`                | restore image + restore album from trash                    |
| 4   | `trash-batch-operations.yaml`       | batch delete to trash + batch permanently delete from trash |
| 5   | `trash-multi-alias-visibility.yaml` | trash one alias → visible in both trash and original album  |
| 6   | `trash-empty-state.yaml`            | empty trash page shows correct empty card                   |

### Unit Tests (3 groups)

| #   | Function                                          | Cases                                                                    |
| --- | ------------------------------------------------- | ------------------------------------------------------------------------ |
| 1   | `resolve_trash_path()` / `resolve_restore_path()` | path → .trash/ equivalent; already in .trash/ → None; custom directory   |
| 2   | `is_in_trash()`                                   | alias in .trash/ → true; outside → false; multi-alias mixed → false      |
| 3   | `resolve_conflict_path()`                         | occupied + skip → None; occupied + rename → `img_1.jpg`; free → original |

## Out of Scope (v0.1)

- **Empty trash** — bulk permanent delete of everything in `.trash/`
- **Trash retention / auto-cleanup**
- **Manual/virtual albums** — deprecated, not implemented
