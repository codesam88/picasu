---
status: in-progress
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1c.

## Feature status

The `assign_album` conflict feature has passing scenario coverage on `main`
(`skip|rename|replace`). The design was explored and implemented on this branch
as `rename|merge` (see Progress below), but that direction was **reverted in
design review**: `merge` is too aggressive for an FS-first gallery where users
may intentionally keep the same file in multiple albums via sync tools.

**Target state:** `OnConflict` = `skip` / `rename` on both assign and upload.
`replace` is dropped (data-loss hazard). No merge deduplication.

## Conflict model

Two modes. The caller chooses per-operation. Backend default for missing field:
`rename` (upload) or 400 (assign).

### Item move (`move_item_into_album`)

- `skip` — if `dest_path` already exists, do nothing. Return 200 with outcome
  `skipped`. The source file and sidecar stay untouched.
- `rename` — move unconditionally. On filename collision pick a unique name
  (`photo-001.jpg`, `photo-002.jpg`, …). Return 200 with outcome `renamedFrom`.
  Never overwrites different bytes.

Sidecar moves with the selected physical file. Sibling aliases sharing the
same hash are left untouched (G1 — selected-alias-only move).

### Sub-album dir move (`move_album_into_album`)

Whole-dir `fs::rename`. On collision with an existing target directory:

- `skip` → no-op, return `skipped`.
- `rename` → `find_unique_path` (`child-001/`), return `renamedFrom`.

No recursive directory merge. The source directory is moved as one unit.

Guarded against moving an album into itself or its own subtree (400).
Rewrites every DB record under the old dir, updates the dir-album cache
prefix, and marks old/new parent albums for stats refresh.

### Upload (`POST /post/upload`)

Upload has its own `save_file` conflict handling:

- absent `on_conflict` → `rename` (unique-suffix, backward-compatible).
- `skip` → discard the uploaded temp file if dest exists.
- `rename` → `find_unique_path`.

### API contract

- `PUT /put/assign_album` — `on_conflict: skip | rename` (required, no
  default). `alias: Option<String>` (required for items, must be absent
  for albums, 400 otherwise).
- `POST /post/upload` — `on_conflict` query param, optional, absent →
  `rename`. Values: `skip`, `rename`.
- Response body: `{ outcome: "moved" | "renamedFrom" | "skipped" }`.

## Routes & interaction vectors

- `PUT /put/assign_album` — accepts image, video, or dir-album hashes. Reached
  from `ItemAlbum.vue` (metadata panel), `ItemEditAlbums.vue` (single menu),
  `ItemBatchEditAlbums.vue` + `AssignAlbumModal.vue` (batch = sequential loop
  of independent calls).
- `POST /post/upload` — direct upload into an album directory, sharing the
  conflict strategies.
- Frontend sequences without a server-side transaction:
  - trash-restore → move (`setTrashed(false)` then `assign_album`),
  - create-album → move (`createDirAlbum` then `assign_album`).

## Open decisions

- **G1 — selected-alias-only move.** Assign operates on the concrete physical
  file: the **selected alias**. A move affects only that alias; sibling aliases
  sharing the same hash stay where they are. `AssignAlbumData` gains a required
  `alias` field (the selected file's path); missing → 400 for items, must be
  absent for albums.

- **G2 — conflict model.** `skip` / `rename` on both assign and upload.
  `replace` removed. Default upload strategy: `rename`. Assign `on_conflict`
  is required with no default.

- **G3 — frontend pass-through.** `assignAlbum()` must send `alias` and
  `onConflict`. `AssignAlbumModal` gains a Skip / Rename radio (Rename
  default). Outcome maps to toast/chip: moved / renamedFrom / skipped.
  Batch flow surfaces per-file results.

## Test coverage

### Existing on main (keep)

- `assign_conflict_default_skip` — no `onConflict` → skip (becomes obsolete
  once `onConflict` is required)
- `assign_conflict_skip_z4` — explicit skip → both files remain
- `assign_conflict_rename_z5` — rename → `photo-001.jpg`, source gone
- `assign_conflict_rename_double_z7` — collision cascade → `photo-002.jpg`
- `assign_conflict_replace_z6` — obsolete (replace removed)
- `assign_album_dir_conflict_skip_zz8` — dir skip → both dirs intact
- `assign_album_dir_conflict_rename_zz9` → `child-001/`, source gone
- `assign_album_dir_conflict_replace_rejected_zza` — obsolete

### Delete (replace scenarios removed)

- `assign_conflict_replace_z6`, `assign_album_dir_conflict_replace_rejected_zza`,
  `upload_conflict_replace` — replace is gone

### Update (default changes from skip to 400)

- `assign_conflict_default_skip` — delete; no default. New test:
  `assign_on_conflict_required` (missing → 400).

### New scenarios

1. `assign_on_conflict_required` — missing `onConflict` → 400.
2. `assign_self_move_noop` — assign into the file's current album → 200
   `skipped` under both modes.
3. `assign_outcome_skipped` — explicit skip on an item that would collide →
   200 `skipped`, source and dest both present.
4. `assign_rename_same_hash_different_name` — rename with same hash, different
   filename → both copies kept in target album.
5. `assign_bypass_alias_required` — item move without `alias` → 400.
6. `assign_album_alias_rejected` — album move with `alias` set → 400.

## Race conditions

- Same item assigned twice concurrently (redb write txn serializes; second call
  errors "not found at recorded path").
- Assign vs delete of the same file.
- Assign while upload into the same target directory.
- TOCTOU between `get_dir_path_for_album` cache resolution and `fs::rename`.
- Sub-album path rewrite racing a concurrent assign inside the moving subtree.

## Security notes

- Request inputs are opaque (`hash`, `album_id`) and resolved server-side —
  no path traversal from request bodies.
- Unknown `album_id` / missing `hash` → 400 (covered).
- No code path writes different bytes over an existing indexed path.

## Implementation plan (commits)

### Phase 1 — API contract + G1

- **C1 — enum + alias.** `OnConflict` → `{Skip, Rename}` (drop Replace and
  Merge, keep `#[default]` as `Rename` for upload backward compat). `AssignAlbumData`
  gains required `alias` + required `on_conflict`. Backend validates alias
  against the record (400 if missing/wrong). Move rewrites only the selected
  alias entry. Stale-check on the selected alias. Return `AssignResult`
  `{ outcome: moved | renamedFrom | skipped }`. utoipa schema updated.

### Phase 2 — Skip behavior

- **C2 — skip path.** In `move_item_into_album`: dest exists + skip → early
  return `Skipped`. In `move_album_into_album`: dir collision + skip → early
  return `Skipped`. Both modes preserve source and dest. Upload: skip discards
  temp file when dest exists.

### Phase 3 — Frontend

- **C3 — frontend.** `assignAlbum()` sends `alias` + `onConflict`. Modal:
  Skip / Rename radio (Rename default). Outcome → toast (moved /
  renamedFrom / skipped). Batch per-file results. Upload: `on_conflict`
  stays absent (defaults to `rename`).

### Phase 4 — Tests

- **C4 — scenarios.** Delete obsolete replace scenarios. Delete
  `assign_conflict_default_skip` (replaced by required-field test). Add new
  scenarios (self-move, outcome-skipped, alias-required, etc.). Update
  surviving `assign_*` callers with `alias` + `onConflict`.

### Phase 5 — Cleanup

- **C5 — probe + dead code.** Remove merge-specific code paths. Remove
  `merge_dedup_upload`. Remove `merge_album_tree`. Remove `DeduplicatedRemoved`
  outcome variant. Keep test-only DB probe only if still useful for G1
  alias-preservation tests; otherwise remove it.

## Progress

- 2026-09-16: G1 resolved — selected-alias-only move, required `alias` field.
- 2026-09-16: G2 initially settled as rename|merge. Explored and implemented
  on this branch. Design review decided merge was wrong direction for FS-first
  gallery. Reverted to skip/rename.
- 2026-09-16–17: Implementation commits C1–C10 implemented the rename|merge
  model. All are candidates for rollback except G1 (selected-alias-only move)
  which is retained.
- 2026-09-18: Plan rewritten for skip/rename target state.
