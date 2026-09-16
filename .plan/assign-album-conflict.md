---
status: in-progress
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1c.

## Feature status

The `assign_album` conflict feature is **implemented and has passing scenario
coverage** (detailed below). What remains: closing the untested corner cases,
two open design decisions that change behavior (G1, G2), and one frontend
pass-through fix (G3).

Code lives in `router/put/assign_album.rs`; the upload route reuses the same
`OnConflict` enum. `OnConflict` = `skip` (default) / `rename` / `replace`.

- Item move (`move_item_into_album`): check `dest_path` exists → `skip`
  returns early (200 no-op), `rename` picks `find_unique_path` (`-001`,
  `-002`, …), `replace` overwrites the existing file.**No hash check** — see
  G2. Sidecar moves to `dest_path.with_extension("xmp")`.
- Sub-album dir move (`move_album_into_album`): whole-dir `fs::rename`;
  `skip`/`rename` behave like items; **`replace` is rejected** (400) —
  recursive-deleting an existing album dir is intentionally unsupported.
  Guarded against moving an album into itself or its own subtree. Rewrites
  every DB record under the old dir (nested albums and all file aliases),
  updates the dir-album cache prefix, and marks old/new parent albums for
  stats refresh.
- Upload route `POST /post/upload?presigned_album_id&on_conflict&auto_rename`
  has its own inline `save_file` conflict handling (strings parsed strictly:
  `skip|rename|replace`, else 400).

## Routes & interaction vectors

- `PUT /put/assign_album` — accepts image, video, or dir-album hashes.
  Reached from `ItemAlbum.vue` (metadata panel), `ItemEditAlbums.vue`
  (single menu), `ItemBatchEditAlbums.vue` + `AssignAlbumModal.vue` (batch =
  sequential loop of independent calls).
- `POST /post/upload` — direct upload into an album directory, with the same
  three conflict strategies.
- Frontend sequences without a server-side transaction:
  - trash-restore → move (`setTrashed(false)` then `assign_album`),
  - create-album → move (`createDirAlbum` then `assign_album`).

## Implemented & passing scenario tests

### assign_album conflict semantics (`assign_conflict_*`)

- **`assign_conflict_default_skip`** — no `onConflict` in body → skip:
  source `import/photo.jpg` and dest `album/photo.jpg` both remain on disk
  (default is safe, no silent overwrite).
- **`assign_conflict_skip_z4`** — explicit `skip` → 200, both files remain.
- **`assign_conflict_rename_z5`** — `rename` → source gone,
  `album/photo-001.jpg` created, existing `album/photo.jpg` untouched.
- **`assign_conflict_rename_double_z7`** — `photo-001` taken → next free
  suffix `photo-002.jpg` used; blockers untouched.
- **`assign_conflict_replace_z6`** — `replace` → 200, source gone, dest
  overwritten (asserted via file presence only — see Gap 2).

### sub-album / directory moves (`assign_album_dir_conflict_*`)

- **`assign_album_dir_conflict_skip_zz8`** — dir-name collision → both
  source and target dirs intact, nothing moved.
- **`assign_album_dir_conflict_rename_zz9`** — collision → moved dir lands
  as `child-001/`, existing `target/child/` untouched, source gone.
- **`assign_album_dir_conflict_replace_rejected_zza`** — `replace` on a dir
  → non-200; nothing moved or deleted.

### reject / error paths

- **`assign_album_rejects_manual_album`** — unknown/bogus album_id → 400.
- **`assign_album_rejects_stale_file_path_j`** — source file removed from
  disk → non-200 (stale-alias guard).

### upload into album (`upload_conflict_*` — all upload the same source file twice)

- **`upload_conflict_skip`** — first upload `replace`, second `skip` → 200;
  `photo.jpeg` exists (**weak**: true even if skip overwrote).
- **`upload_conflict_rename`** — `rename` on second → both `photo.jpeg` and
  `photo-001.jpeg` exist.
- **`upload_conflict_replace`** — both `replace` → 200; `photo.jpeg` exists
  (same content both times).

### surrounding assign behavior (not conflict-specific)

`xmp_sidecar_moves_with_file_z2` (sidecar follows move),
`complex_tags_survive_assign_y` (tags/metadata preserved),
`assign_album_moves_sub_album_directory_zz7` (nested subtree + rewritten
paths), `assign_album_moves_multiple_independent_albums_zzb`,
`assign_album_updates_album_tree_parent_zza`,
`assign_album_move_clears_source_grid_zzc` (grid cache),
`album_visible_via_get_data_after_assign_q`,
`image_serving_survives_album_move_v`, `album_membership_singular_i`,
`assign_multiple_files_to_album_z8`.

### frontend UI (Playwright)

`assign-photo-to-album.yaml` — single photo moved via the sidebar modal;
asserts success toast + sidebar album chip reflects destination. **No**
batch, trash-restore→move, or create-album→move flow is covered.

## Suspected gaps & untested corner cases

1. **Multi-alias record move (data loss).** Dedup produces records with >1
   alias (`tasks/actor/deduplicate.rs`). `move_item_into_album` replaces the
   whole alias list with a single path (`assign_album.rs` move_item),
   moving only `alias[0]`'s file and **dropping every other alias** — the
   remaining physical copies stay on disk but the DB stops referencing them.
   No scenario covers it, and the scenario DSL cannot assert `alias[]`/
   `album()` contents (only file existence and response JSON), so DB-level
   consequences are invisible to the suite.
2. **`replace` across different hashes (integrity).** Hash = BLAKE3 of
   content (`process/hash.rs`). Replace `fs::rename`s new bytes over the
   destination while the DB still maps the _old_ hash to that path. Both
   replace tests assert file presence only — never that the old record still
   advertises a hash that no longer matches the bytes at its path, nor what
   happens to the old record's thumbnail.
3. **Replace clobbers the dest record's `.xmp` sidecar.** On replace the
   source sidecar overwrites `dest.xmp`; the replaced record's sidecar
   (tags, description, rating) is destroyed. Untested.
4. **Self-descendant guard via API.** `target_dir.starts_with(&source_dir)`
   (move_album) has no direct test — only the client-side ancestor +
   descendant _selection_ is covered (`assign_album_moving_ancestor_then_descendant_extracts`).
   A direct PUT of an album into its own subtree (expect 400, untouched) is
   unasserted.
5. **Dest == current path (self-move no-op).** Moving a file into the album
   it already lives in is a 200 no-op by design (`base_dest != current_path`
   branch); the UI disables it but the API path is untested.
6. **Stale album-directory cache.** The `album_dir.is_dir()` → 400 guard is
   untested; the stale test only covers a missing _source file_, not a
   missing _album directory_.
7. **Invalid `on_conflict` value.** Assign route: serde enum rejection
   (expected 422); upload route: strict string-parse 400. Both untested.
8. **Dir-vs-file and rename-onto-nonempty-dir collisions.** POSIX errors
   surface as 500 (GenericFile upgrade). Untested.
9. **Cross-device / permissions (EXDEV, EACCES, read-only FS).** Upload side
   has `upload_readonly`; the assign side has none. A mid-operation failure
   leaves unrecovered temp/partial state.
10. **Concurrency.** Suite is serialized by `TEST_SERIAL_GUARD`; no test
    covers assign-vs-assign on the same item, assign-vs-delete, or
    assign-while-upload into the same dir.
11. **Frontend/UI.** Batch assign, trash-restore→move (incl. the half-failed
    state where untrash succeeded but move failed), create-album→move,
    batch partial-failure recovery, and the silent-skip misreport (G3) are
    all uncovered in Playwright.

## Open decisions

- **G1 — multi-alias move.** On assigning a record that has several aliases
  (dedup duplicates), what should happen? Refuse the move / move every
  physical copy / keep the other aliases pointing where they are. Currently
  it silently drops them (data loss).
- **G2 — replace semantics.** Enforce the original spec ("replace only if
  the hashes match, else error") or explicitly document that replace is a
  byte-level overwrite that leaves the old record flat? If kept, the old
  record's path/thumbnail/sidecar handling must be defined.
- **G3 — frontend on_conflict pass-through.** The UI's `assignAlbum()` omits
  `onConflict`, so a collision silently no-ops (200) while the store
  optimistically shows the item moved. Either send a strategy from the UI or
  surface a 4xx so the user is told why nothing moved.

## Race conditions to cover

- Same item assigned twice concurrently (redb write txn serializes; the
  second call likely errors "not found at recorded path" — make it a test).
- Assign vs delete of the same file; assign vs in-flight upload into the
  same target directory.
- TOCTOU between `get_dir_path_for_album` cache resolution and `fs::rename`
  when the album directory is renamed/deleted in between (partly covered by
  stale-path rejection).
- Sub-album path rewrite racing a concurrent assign of an item inside the
  moving subtree.

## Security notes

- Request inputs are opaque (`hash`, `album_id`) and resolved server-side —
  no path traversal from request bodies.
- Unknown `album_id` / missing `hash` → 400 (covered).
- G2 is the real integrity risk: replace lets a content hash advertise bytes
  that no longer match it.

## Refactor candidate: share one file-landing helper

The two handlers cannot merge — upload is a multipart batch with sanitize/
preflight/index, assign moves a single already-indexed record. But the
conflict resolution block is duplicated, including a second copy of the
unique-name finder:

- `save_file` (`post_upload.rs`) — tmp path → conflict resolution → rename,
  own `find_unique_upload_path`.
- `move_item_into_album` (`assign_album.rs`) — alias path → conflict
  resolution → rename, own `find_unique_path`.
- `OnConflict` enum already shared.

Extract one helper both call, e.g.
`place_file(src, dest_dir, filename, conflict) -> Option<PathBuf>` returning
`None` on skip; apply `rename`/`replace` once; single `find_unique_path`
(delete `find_unique_upload_path`). Also share the album target-dir
resolution (`get_dir_path_for_album` + the is-a-directory check). Low blast
radius; best done together with the G2 decision. Spin off as its own ticket
when scheduled.

## Progress

- 2026-09-16: full investigation appended — vectors, per-scenario test
  inventory, Gap 1–11 corner cases, race and security notes, G1–G3 open
  decisions, `place_file` unification candidate. Corrected from a first
  draft: upload conflict scenarios do exist but are same-content-only and
  skip is weakly asserted. Plan rewritten plain after an initial
  over-compression.
