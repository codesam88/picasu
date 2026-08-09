# Restore as Assign-Album (Trash Page)

## Context

The trash page (`/trashed`, `basicString: 'trashed:true'`) renders the shared
`GalleryMain` grid, so both single-item menus and the batch (multi-select) menu
are available there. "Trashed" is _inferred from the on-disk path_: an item is
trashed iff every alias (or an album's `dir_path`) lives under `image_home/.trash`
(`expression.rs` trashed case). Trashing rewrites the alias path into `.trash/`
but preserves the `album` membership field (`delete.rs`).

Today the trash page offers overlapping, confusing actions:

- **Restore** (`ItemRestore.vue`) → `GET /put/restore-from-trash`, always moves the
  file back to its exact pre-trash path (`image_home/<relative>`).
- **Assign Album** (`ItemEditAlbums.vue` / `ItemBatchEditAlbums.vue`) → `PUT
/put/assign_album`, moves the item into a chosen album's directory. In the
  trash page this moves it _out of_ `.trash/` into an arbitrary album — i.e. it
  already functions as "restore to an arbitrary album."
- Batch actions (**favorite, archive, batch edit tags/albums**) apply trashed
  items.

The two move paths overlap; the menu semantics are confusing.

## Goal

Make the trash page offer one coherent recovery action. **Restore opens the
album-selection dialog (assign album) prefilled with the item's original album**;
the user confirms to restore into that album or picks another album to restore
into an arbitrary location. Multi-select restores multiple files to one chosen
target album. Favorite/archive/batch-edit/tags actions are not offered in the
trash context.

Unify file/album moving behind a single central **assign-album** endpoint so
access control and move semantics are enforced in one place later.

## Decisions

1. **Reuse `PUT /put/assign_album` as the only move endpoint.** It already moves
   an item from `alias[0].file` (whatever path, including `.trash/`) into the
   target album directory, rewrites the alias, moves the `.xmp` sidecar, sets
   album membership, and handles `on_conflict` skip/rename/replace. Once moved
   out of `.trash/`, the item is no longer inferred "trashed."
2. **Remove `restore-from-trash` backend.** Superseded by `assign_album`.
3. **Remove the old `ItemRestore.vue` direct call.** Restore becomes a
   modal-opening action reusing the `AssignAlbumModal`.
4. **Multi-select restore:** kept and driven by `assign_album` in batch mode
   (one target album for all selected items).
5. **Access control:** nothing new now; the consolidation is the precondition for
   a later centralized `enforcement` on the single move endpoint.

## Backend Changes

- Delete `backend/src/router/put/restore_from_trash.rs`; remove its module
  declaration and route registration from `put/mod.rs`.
- No functional change to `assign_album.rs`. Verify (add a test if missing) that
  moving a trashed item out of `.trash/` into an album dir flips the item out of
  the `trashed:true` filter (regression: alias path no longer starts with
  `.trash/`).

## Frontend Changes

### Menu items

Single (`SingleMenu.vue`), batch (`BatchMenu.vue`), and album (`AlbumMenu.vue`)
menus, in the trash context only:

- Keep: View Original, Download, **Restore**, Permanently Delete (single/album).
- Remove in trash context: Edit Tags, Assign Album (single), batch Favorite /
  Archive / Batch Edit Tags / Batch Edit Albums, Scan Album, Rotate Image.

The Restore item in trash context is the `AssignAlbum` modal opener (rebranded),
not the old direct call.

### AssignAlbumModal.vue

Extend to support a "restore mode" entered from the trash context:

- Accept an optional prop / mode flag (e.g. `restore: boolean`) set true when
  opened from `/trashed`.
- **Prefilled default target:**
  - Single mode: the item's original album from `data.album`
    (`currentAlbumId`, which in the trash route resolves to the member — album
    preserved through trash).
  - Batch/multi: the **first selected image's** original album.
- **Enable Move button on same-album selection in restore mode.** Currently
  disabled when `selectedAlbumId === currentAlbumId` to avoid a no-op move in a
  normal album. In restore mode this an explicit move out of the trash and must
  be enabled.

Note on single-mode route resolution: `currentAlbumId` is derived via
`getHashIndexDataFromRoute(route)`, which requires `route.params.hash`. The trash
grid route resolves the item through the shared data store, so this works for
single-item restore. For robustness the restore mode should fall back to the
first item of the selection when route params are unavailable.

- Title/label: show "Restore to Album" in the restore mode (icon `mdi-restore`)
  vs "Move to Album" (`mdi-folder-move`) otherwise.
- Otherwise identical behavior: album tree search, create-new-album, submit
  dispatches `assignAlbum` (single) or the batch loop.

### API call and on_conflict

`assignAlbum.ts` currently sends only `{ hash, albumId }` — `on_conflict` is left
at the back-end default `skip`. For restore that is wrong: if a same-named file
already exists in the target album, restore would silently do nothing.

- Extend `assignAlbum` to accept an `onConflict` argument (`skip|rename|replace`)
  and include it in the request body.
- The AssignAlbumModal's **normal** mode keeps the existing default (skip) so
  ordinary album-move behavior is unchanged.
- The **restore** mode passes `rename` so a trashed file restored into an album
  that already holds a same-named file gets `-001` appended rather than being
  skipped or overwritten.

## Confirm the source album lookup

`assign_album` resolves the album's `dir_path` from the `get_dir_path_for_album`
directory cache. On restore into the original album the file physically lives in
`.trash/`, and the target album's directory must exist on disk. Edge case: the
original album's folder was also trashed/deleted — the assign modal will reject it
with the existing "directory no longer has on disk — re-index" error. Document
this in the result.

## Non-Goals

- No new backend move endpoint (reuse assign_album).
- Trashed state stays path-inferred; no persistent "origin" field is added.
- Frontend album assignment UI logic (search/tree/create) unchanged except the
  restore-mode prefill + enable rule.

## Testing

- Backend: unit test moving a `.trash/`-resident image into an album dir ->
  trashed:false; assert `restore-from-trash` route removed from OpenAPI.
- Frontend: component/spec test that AssignAlbumModal in restore mode prefills
  to `data.album`, enables Move on same-album, and dispatches `assignAlbum`.
- E2E: trash page single restore → item leaves `/trashed`; multi-select restore →
  all leave; assigned to the chosen album.
