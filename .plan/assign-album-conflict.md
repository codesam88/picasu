---
status: in-progress
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1c.

## Feature status

The `assign_album` conflict feature has passing scenario coverage for the
path-primary asset model. Each physical file is an independent asset identified
by `asset_id`; same-content files remain independent and are grouped through
`DUPE_INDEX`.

**Target state:** `OnConflict` = `skip` / `rename` on both assign and upload.
`replace` is dropped (data-loss hazard). Identity-based merge is not supported.

## Conflict model

Two modes. The caller chooses per-operation. Backend default for missing field:
`rename` (upload) or 400 (assign).

### Item move (`move_item_into_album`)

- `skip` — if `dest_path` already exists, do nothing. Return 200 with outcome
  `skipped`. The source file and sidecar stay untouched.
- `rename` — move unconditionally. On filename collision pick a unique name
  (`photo-001.jpg`, `photo-002.jpg`, …). Return 200 with outcome `renamedFrom`.
  Never overwrites different bytes.

Sidecars move with the physical file identified by `asset_id`. Other assets with
the same content hash are left untouched.

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

- `PUT /put/assign_album` — `asset_id` and `on_conflict: skip | rename`
  (required, no default).
- `POST /post/upload` — `on_conflict` query param, optional, absent →
  `rename`. Values: `skip`, `rename`.
- Response body: `{ outcome: "moved" | "renamedFrom" | "skipped" }`.

## Routes & interaction vectors

- `PUT /put/assign_album` — accepts image, video, or dir-album asset IDs. Reached
  from `ItemAlbum.vue` (metadata panel), `ItemEditAlbums.vue` (single menu),
  `ItemBatchEditAlbums.vue` + `AssignAlbumModal.vue` (batch = sequential loop
  of independent calls).
- `POST /post/upload` — direct upload into an album directory, sharing the
  conflict strategies.
- Frontend sequences without a server-side transaction:
  - trash-restore → move (`setTrashed(false)` then `assign_album`),
  - create-album → move (`createDirAlbum` then `assign_album`).

## Decisions

- **G1 — asset-specific move.** Assign operates on the one physical file
  identified by `asset_id`. Same-content assets remain independent.

- **G2 — conflict model.** `skip` / `rename` on both assign and upload.
  `replace` removed. Default upload strategy: `rename`. Assign `on_conflict`
  is required with no default.

- **G3 — frontend pass-through.** `assignAlbum()` sends `assetId` and
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

## Race conditions

- Same item assigned twice concurrently (redb write txn serializes; second call
  errors "not found at recorded path").
- Assign vs delete of the same file.
- Assign while upload into the same target directory.
- TOCTOU between `get_dir_path_for_album` cache resolution and `fs::rename`.
- Sub-album path rewrite racing a concurrent assign inside the moving subtree.

## Security notes

- Request inputs are opaque (`asset_id`, `album_id`) and resolved server-side —
  no path traversal from request bodies.
- Unknown `album_id` / missing `asset_id` → 400 (covered).
- No code path writes different bytes over an existing indexed path.

## Implementation plan (commits)

### Phase 1 — API contract + asset identity

- **C1 — enum + asset ID.** `OnConflict` → `{Skip, Rename}`. `AssignAlbumData`
  requires `asset_id` and `on_conflict`. The move resolves the canonical path
  from the asset record. Return `AssignResult` `{ outcome: moved |
renamedFrom | skipped }`. Update the utoipa schema.

### Phase 2 — Skip behavior

- **C2 — skip path.** In `move_item_into_album`: dest exists + skip → early
  return `Skipped`. In `move_album_into_album`: dir collision + skip → early
  return `Skipped`. Both modes preserve source and dest. Upload: skip discards
  temp file when dest exists.

### Phase 3 — Frontend

- **C3 — frontend.** `assignAlbum()` sends `assetId` + `onConflict`. Modal:
  Skip / Rename radio (Rename default). Outcome → toast (moved /
  renamedFrom / skipped). Batch per-file results. Upload: `on_conflict`
  stays absent (defaults to `rename`).

### Phase 4 — Tests

- **C4 — scenarios.** Delete obsolete replace scenarios. Delete
  `assign_conflict_default_skip` (replaced by required-field test). Keep
  self-move, conflict-outcome, and independent-duplicate scenarios. Update
  surviving `assign_*` callers with `assetId` + `onConflict`.

### Phase 5 — Cleanup

- **C5 — probe + dead code.** Keep only path-primary duplicate-group and
  asset-record probes needed by tests. Remove obsolete identity-merge references
  from plans and generated references.

## Progress

- 2026-09-16–18: Conflict handling settled on `skip` / `rename`; identity-based
  merge and overwrite behavior are not supported.
- 2026-09-23: Plan aligned with path-primary asset identity. The move contract
  uses `asset_id`; duplicate behavior is covered independently through
  `DUPE_INDEX` scenarios.
