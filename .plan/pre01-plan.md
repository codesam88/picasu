# pre01: First-Release Feature Plan

## Overview

Three interlocking areas for first release: file lifecycle correctness, metadata
read from files, and metadata write-back to XMP sidecars. A fourth cross-cutting
concern — rating — falls out of the metadata work.

---

## Area 1: File Lifecycle

### 1a. Delete actually deletes files from disk

**Current state**: "Delete" in the frontend sets `is_trashed=true` via
`PUT /put/edit_flags`. `DELETE /delete/delete-data` only removes the DB record
via `FlushTreeTask::remove`; the original file, thumbnail, and (eventually) sidecar
remain on disk.

**Gap**: No code path ever calls `fs::remove_file`. Thumbnails in `DATA_HOME` are
also orphaned. Design says "user may delete files via API" — implying actual
removal.

**What's needed**:

- Two-step delete UX: "trash" (soft, existing) → "confirm delete from disk"
  (hard, missing). TrashedPage already exists; needs a "permanently delete"
  action that calls the current endpoint.
- `DELETE /delete/delete-data` (or a new endpoint) must:
  1. For each alias path, `fs::remove_file` the original
  2. `fs::remove_file` the `.xmp` sidecar if it exists (once sidecars are
     implemented)
  3. `fs::remove_file` the compressed thumbnail at `compressed_path(hash)`
  4. Remove from DB (already done)
- Handle multi-alias case: only remove from disk when removing the last alias;
  for earlier aliases only remove that alias path from the `alias[]` list.
- `stale-dir-album-cache.md` applies here too: `DIR_ALBUM_CACHE` entries for
  deleted dirs should be evicted.

### 1b. File removal events in the watcher

**Current state**: `start_watcher.rs` handles `Create` and `Modify` events;
`Remove` events fall through to `_ => {}` (ignored).

**Gap**: If a file is deleted externally (outside the API), it stays in the DB
as a stale record with a broken alias. Design calls for detecting and removing
these.

**What's needed**:

- Handle `EventKind::Remove(_)` in the watcher: look up the path in `DATA_TABLE`
  (scan aliases), remove the alias from the record, and if no aliases remain,
  remove the whole record + thumbnail.
- On manual album indexing (`POST /post/index/album`), the sweep that happens
  after scanning for new files should also check existing DB records under the
  target path for dead aliases (design.md: "consult the DB for the selected
  target path and check if all known files exist").

### 1c. assign_album conflict handling

**Current state**: `assign_album` renames the file to `album_dir/filename`. If a
file already exists at that path, `fs::rename` silently overwrites on Linux.
There is no `on_conflict` parameter.

**Gap**: Design says auto-rename if target exists with different hash (else skip),
auto-replace if same hash (else skip). No conflict strategy is implemented.

**What's needed**:

- Add `on_conflict: "rename" | "replace" | "skip"` to `AssignAlbumData`.
  Default: `"skip"` (safe) so current callers don't silently overwrite.
- Before `fs::rename`: check if `dest_path` exists. If so:
  - `"skip"`: return early (or 409)
  - `"replace"`: only if hashes match, else error
  - `"rename"`: append `_1`, `_2`, … until a free name is found
- Move `.xmp` sidecar alongside the file (once sidecars exist — placeholder
  now, hook in Area 3).

### 1d. Upload conflict handling

**Current state**: `post_upload` renames from tmp to final path with no conflict
check — `fs::rename` overwrites silently if the file already exists.

**What's needed**: same `on_conflict` parameter pattern as 1c.

### 1e. Verify file actions (verify-file-actions.md)

E2E test coverage for all of the above. Scenarios:

- Watcher: new file discovered, duplicate hash → alias added, file deleted
  externally → stale record cleaned up
- Manual indexing: sweep detects and removes dead aliases
- API upload with each conflict strategy
- API move with each conflict strategy
- API delete: trash → confirm → file gone from disk + DB + thumbnail

---

## Area 2: Metadata Read

### 2a. Replace XMP keyword stub with a real parser

**Current state**: `process/xmp.rs` is a raw byte scan for `<dc:subject>` only.
Only `dc:subject` (tags/keywords) is extracted. No sidecar discovery. No other
XMP fields (description, rating, title, etc.) are read.

`process/exif.rs` reads EXIF into a flat `BTreeMap<String, String>` keyed by
tag name strings. The frontend shows only `Make` and `Model` from this map.

Neither populates `ObjectSchema.description`, `ObjectSchema.tags`, or
`ObjectSchema.is_favorite` at index time — these fields only get values if the
user sets them via API after indexing.

**Crate choice**: `metadata.md` recommends `rexiv2` (GObject/libgexiv2 wrapper,
reads EXIF+IPTC+XMP, supports sidecar discovery) or `xmpkit` (pure Rust, full
XMP data model). `kamadak-exif` is already in use for EXIF. For first release,
the simplest path is `rexiv2` (reads all three formats + sidecars via one API)
or `kamadak-exif` + `xmpkit`. Given the system-lib dependency of `rexiv2`, pure
Rust is preferable for portability; use `kamadak-exif` for EXIF + `xmpkit` for
XMP.

**What's needed at indexing (`IndexTask`)**:

1. Discover sidecar: for `photo.jpg` look for `photo.xmp` in the same dir.
   Sidecar XMP overrides embedded.
2. Read fields in priority order per `metadata.md §5`:
   - `tags` ← `dc:subject` (XMP/sidecar) → IPTC `2:25` Keywords (fallback)
   - `description` ← `dc:description` (XMP/sidecar) → IPTC `2:120` Caption →
     EXIF `ImageDescription` (fallback)
   - `title` ← `dc:title` (XMP/sidecar) → IPTC `2:05` ObjectName (fallback)
     _(title is not in `ObjectSchema` yet — see Area 4)_
   - `rating` ← `xmp:Rating` (XMP/sidecar only — no EXIF/IPTC equivalent)
     _(rating field not in model yet — see Area 4)_
   - Capture date ← `photoshop:DateCreated` / `xmp:CreateDate` → `DateTimeOriginal`
     (already used in `abstract_data.rs`)
   - GPS ← EXIF GPS IFD → `exif:GPSLatitude`/`exif:GPSLongitude` (XMP mirror)
3. Write extracted fields into `ObjectSchema` before DB flush. Rule: **do not
   overwrite user-set values** — if `description` is already non-empty (was set
   via API), do not overwrite with the file-extracted value. Same for `tags`:
   file-extracted tags are added (union) to any existing API-set tags.
   _(This requires tag provenance tracking to be precise — see Area 3.)_

### 2b. Surface more EXIF in the frontend

Frontend `MetadataContent.vue` only shows `Make` and `Model`. The flat
`exif_vec` BTreeMap already carries many more fields; they just aren't displayed.

**What's needed** (UI only, no backend change):

- `ItemExif.vue`: expand to show Date taken, resolution, focal length, f-number,
  ISO, exposure time, GPS (link to map). Keep it compact/collapsible.
- `ItemDate.vue`: currently uses the backend-computed `update_at` or EXIF date —
  verify it shows the capture date, not the indexing date.

---

## Area 3: XMP Sidecar Write-back

### 3a. Write `.xmp` sidecar on metadata changes

**Current state**: `edit_tag.rs`, `edit_description.rs`, `edit_flags.rs` all
update the DB record and flush — no file is written to disk.

Design.md: "whenever metadata is changed via the API/frontend, the backend will
create/update a corresponding sidecar XMP file."

**What's needed**:

- After any DB update in `edit_tag`, `edit_description`, `edit_flags` (for
  `is_favorite`), write/update `{basename}.xmp` alongside the original file.
- Sidecar naming convention: `photo.jpg` → `photo.xmp` (not `photo.jpg.xmp`).
  Follows Adobe/Lightroom convention per `metadata.md §4`.
- XMP packet contents: write only the fields managed by the app:
  `dc:subject` (tags), `dc:description`, `xmp:Rating` (favorite → 1 or 0 star?
  or a separate `xmp:Label`), leaving existing embedded XMP in the original
  untouched.
- For multi-alias items: write the sidecar alongside `alias[0]` (primary path).
- Crate: `xmpkit` (pure Rust, full XMP data model) or `xmp-writer` (write-only,
  simpler). For first release, `xmp-writer` is sufficient — we only write, not
  round-trip.

### 3b. Move sidecar when file is moved

- `assign_album`: after `fs::rename(src, dst)`, also
  `fs::rename(src_sidecar, dst_sidecar)` if the source sidecar exists.
- If the destination sidecar already exists: merge (union tags, prefer sidecar
  description if non-empty) — or for simplicity, overwrite with source sidecar
  in v1.

### 3c. Delete sidecar when file is deleted

- `delete_data` (permanent delete, from 1a): after `fs::remove_file(alias_path)`,
  also `fs::remove_file(sidecar_path)` if it exists.

### 3d. Tag provenance

**Current gap**: `tags` in `ObjectSchema` is a flat `HashSet<String>` with no
record of whether each tag came from file metadata (XMP/IPTC at index time) or
was added by the user via API. This means:

- Re-indexing a file can add duplicate tags (or need a merge strategy).
- We can't distinguish "user removed a tag that was in the file" from "tag was
  never there."

**Minimal v1 approach**: On indexing, extracted tags are unioned into the
existing set. On sidecar write, the full `tags` set is written to `dc:subject`
regardless of provenance. This means removing a tag via API and then
re-indexing will re-add it from the file — acceptable for v1, noted as a known
limitation.

**Future**: Add `tag_source: ContentDiscovered | UserSet` per-tag (see
`scrub-endpoint.md` notes on this).

---

## Area 4: Model Extensions

### 4a. Rating field

`ObjectSchema` has no rating. Design.md lists rating under Photo Properties.
XMP `xmp:Rating` is a 0–5 integer.

**What's needed**:

- Add `pub rating: Option<u8>` to `ObjectSchema`.
- Populate at indexing from `xmp:Rating` (Area 2).
- Add `PUT /put/edit_rating` endpoint (same pattern as `edit_flags`).
- Write to sidecar on change (Area 3).
- Display in `MetadataContent.vue` as a star widget (read-only in share mode).

### 4b. Title field

`ObjectSchema` has `description` but no `title`. Design mentions "tags/labels,
favorite, description, rating" — title is less critical but `dc:title` is a
first-class XMP field and the frontend album has a `title` field.

**Decision**: defer title for images in v1 — description is sufficient. Albums
already have `title`. Revisit if needed.

---

## Area 5: Stale DIR_ALBUM_CACHE

Already tracked in `stale-dir-album-cache.md`. `assign_album` and delete both
depend on the cache being consistent. Fix at startup (load and verify entries)
and at request time (return clear 400 if stale, not a silent move to a
non-existent directory).

---

## Gaps Not in Any Existing Plan Item

| Gap                                     | Action                                      |
| --------------------------------------- | ------------------------------------------- |
| Delete does not remove files from disk  | New item or expand `verify-file-actions.md` |
| Watcher ignores `Remove` events         | New item or expand `verify-file-actions.md` |
| `assign_album` has no conflict handling | New item or expand `verify-file-actions.md` |
| Upload has no conflict handling         | New item or expand `verify-file-actions.md` |
| Rating field missing from model         | Expand `xmp-sidecar-metadata.md`            |
| Sidecar move/delete alongside file      | Expand `xmp-sidecar-metadata.md`            |

---

## Implementation Order

```
1. Area 4a: rating field in ObjectSchema (model change, needed by 2 and 3)
2. Area 2a: proper metadata read at index time (xmpkit + sidecar discovery)
2. Area 2b: surface more EXIF in frontend
3. Area 3a: XMP sidecar write-back on tag/description/favorite/rating changes
3. Area 3b: sidecar moves with file in assign_album
4. Area 1c: assign_album conflict handling (rename/replace/skip)
4. Area 1d: upload conflict handling
5. Area 1a: delete actually removes files from disk + thumbnails + sidecars
5. Area 1b: watcher handles Remove events
6. Area 5: stale DIR_ALBUM_CACHE fix
7. Area 1e: E2E test coverage for all of the above
```

Areas 2+3 are tightly coupled (read and write use the same sidecar path logic)
so they should land in the same branch. Areas 1c+1d share the same
`on_conflict` logic. Area 1a+1b are the destructive operations and should have
E2E coverage before landing.

---

## Plan Items to Update

- `xmp-sidecar-metadata.md`: un-defer; expand to cover Areas 3 + 4a (rating)
- `test-exif-xmp-handling.md`: promote to `open`; expand to cover Area 2a
- `verify-file-actions.md`: expand to cover Areas 1a–1d explicitly
- `stale-dir-album-cache.md`: unchanged (open, high — prerequisite for 1a/1c)
- **New item needed**: "file-lifecycle-gaps.md" or fold the new gaps (delete
  from disk, watcher Remove, conflict handling) into `verify-file-actions.md`
