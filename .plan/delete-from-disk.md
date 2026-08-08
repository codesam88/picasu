---
status: done
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1a.

## Context

Delete lifecycle redesigned as filesystem-based two-stage trash. First delete
moves files to a configurable `.trash/` directory (filesystem rename). Second
delete from trash permanently removes files and DB records. Untrash restores
files to original location. No `is_trashed` flag — trash state derived from
filesystem location.

Spec: `docs/superpowers/specs/2026-08-08-delete-lifecycle-design.md`

## Tasks

### Config

- [ ] Add `trash_enabled: bool` (default true) and `trash_directory: String`
      (default ".trash") to `AppConfig`, `TomlGallery`, both `From` impls,
      `PartialUpdateConfigRequest`, `ConfigResponse`, handlers, test helpers.

### Delete endpoint (`DELETE /delete/delete-data`)

- [ ] New request shape: `DeleteItem { index, alias_path }`, `DeleteList { delete_list, timestamp }`.
- [ ] Trash-move (album): `fs::rename` to `.trash/`, `rewrite_paths_under()`,
      `rewrite_dir_album_cache_prefix()`, mark parent albums for update.
- [ ] Trash-move (image/video): `fs::rename` to `.trash/`, update alias entry.
- [ ] Permanent-delete (album cascade): enumerate members, delete all files +
      sidecars + thumbnails, remove DB records, `fs::remove_dir_all`, evict cache.
- [ ] Permanent-delete (image/video partial alias): remove targeted alias, delete
      file + sidecar. If last alias: delete thumbnail + DB record.

### Untrash endpoint (`PUT /put/restore-from-trash`)

- [ ] New endpoint: `RestoreItem { index, alias_path }`, `RestoreList`.
- [ ] Restore (album): `fs::rename` from `.trash/` back, `rewrite_paths_under()`,
      `rewrite_dir_album_cache_prefix()`.
- [ ] Restore (image/video): `fs::rename` back, update alias entry.
- [ ] Conflict handling: `on_conflict` parameter (skip/rename/replace).

### Filter system

- [ ] Remove `Expression::Trashed` from `expression.rs` and lexer.
- [ ] Update all page filter strings from `trashed:false` to `not(album:.trash)`
      and from `trashed:true` to `album:.trash`.

### Data model

- [ ] Remove `is_trashed` from `ObjectSchema`.
- [ ] Remove `set_trashed()` from `AbstractData`.

### Watcher

- [ ] Ignore events under `.trash/` prefix.

### Frontend

- [ ] `ItemDelete.vue`: resolve aliasPath, send `(index, aliasPath)` pairs.
- [ ] `ItemPermanentlyDelete.vue`: same, plus `refreshGalleryAfterMutation()`.
- [ ] `ItemRestore.vue`: call `PUT /put/restore-from-trash`.
- [ ] Update page filter strings (7 pages).

### Tests (API E2E — 12 scenarios)

- [ ] `trash_and_permanent_delete_image.yaml`
- [ ] `trash_and_permanent_delete_album.yaml`
- [ ] `trash_multi_alias.yaml`
- [ ] `permanent_delete_alias_preserves_record.yaml`
- [ ] `trash_disabled_permanent_delete.yaml`
- [ ] `custom_trash_directory.yaml`
- [ ] `trash_read_only_mode_blocked.yaml`
- [ ] `trash_edge_cases.yaml`
- [ ] `untrash_image_and_album.yaml`
- [ ] `untrash_conflict_handling.yaml`
- [ ] `untrash_not_in_trash_rejected.yaml`
- [ ] `watcher_ignores_trash_events.yaml`

### Tests (Playwright UI — 6 scenarios)

- [ ] `delete-to-trash-flow.yaml`
- [ ] `trash-permanently-delete.yaml`
- [ ] `trash-restore.yaml`
- [ ] `trash-batch-operations.yaml`
- [ ] `trash-multi-alias-visibility.yaml`
- [ ] `trash-empty-state.yaml`

### Tests (Unit — 3 groups)

- [ ] `resolve_trash_path()` / `resolve_restore_path()`
- [ ] `is_in_trash()`
- [ ] `resolve_conflict_path()`
