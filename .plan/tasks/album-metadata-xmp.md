---
status: in-progress
type: feature
priority: medium
area: backend
---

## Custom album metadata via `.albuminfo.xmp` sidecar

Directory-backed (dir_album) albums currently only get a name derived from the
directory path (`prettify_dir_name`), plus a DB-only, disk-persisted-nowhere
title override (`PUT /put/set_album_title`). There is no description, keyword,
or date customization, and any customization is lost when a directory is
renamed/moved (dir-album identity is keyed by live path — a rename is seen by
the watcher as remove-old + create-new, generating a fresh album ID).

Goal: let users customize title, description, keywords, and a date override
for a dir-album, persisted to a `.albuminfo.xmp` sidecar file inside the
album's directory (mirroring the existing per-photo `.xmp` sidecar pattern),
so metadata survives directory moves without needing stable album IDs.

### Design

Discovered while researching: `set_user_defined_description`, `edit_tag`, and
`edit_rating` already operate generically on any `AbstractData` (including
albums) via the shared `ObjectSchema` fields, and already call
`write_sidecar_for`. They silently no-op for albums today only because
`write_sidecar_for` derives the sidecar path from the item's alias file, and
albums have none (`AbstractData::Album(_).alias()` is always `&[]`).

1. **XMP module** (`backend/src/process/xmp.rs`, `xmp_write.rs`): add `dc:title`
   (currently missing — only `dc:subject`/`dc:description`/`xmp:Rating` exist)
   and a new `dc:date` (ISO 8601 text) for the custom date override.
2. **`write_sidecar_for`**: add an `AbstractData::Album` branch resolving the
   sidecar path as `dir_path.join(".albuminfo.xmp")` instead of
   alias-derived. No-ops (`Ok(())`) when `dir_path` is `None` (manual,
   non-directory albums have nowhere on disk to write).
3. **`set_album_title`** (`backend/src/router/put/edit_album.rs`): currently
   DB-only — extend to also call the now album-aware `write_sidecar_for`,
   matching the photo/video edit endpoints.
4. **New `set_album_date` endpoint**: new `custom_date: Option<String>` field
   on `AlbumMetadata`, single-field edit endpoint following the same pattern
   as `set_album_title`.
5. **Hydration on (re)creation**: `write_album_to_db`
   (`backend/src/process/dir_album.rs`) is the single choke point building a
   dir-album's initial record (first discovery, or a "new" album ID after a
   directory rename). Before defaulting to `prettify_dir_name`, check for
   `.albuminfo.xmp` in the directory and use any fields it has (title,
   description, tags, rating, date), defaulting the rest. This is what makes
   custom metadata survive a directory move — the file travels with the
   directory and gets re-read regardless of the album getting a new ID.
6. **Frontend**: `DisplayAlbum.vue` is dead code (confirmed unreachable both
   before and after the `feat/ui-overlays` route-flattening — a single click
   on an album-type grid item always routed directly to `/album/:hash`,
   never through the fullscreen viewer where `DisplayAlbum.vue` would have
   mounted). Remove/repurpose it. Add an "Album Info" item to
   `BatchMenu.vue`'s single-select context menu (shown when the one selected
   item is album-type), opening a new modal with title/description/keywords/
   date fields — all optional, "unintrusive" per user request (no forced
   fields, reachable only via explicit menu action, not the default view).
   Saved per-field via the endpoints above, matching the existing
   auto-save-on-edit pattern used elsewhere (not one big "Save" button).

Rejected alternative: a single new unified `PUT /put/edit_album_info`
endpoint saving all fields at once. Would duplicate logic already in three
working endpoints and break from the established one-endpoint-per-field
convention used everywhere else in the app.

### Verification

1. Backend unit tests for `dc:title`/`dc:date` parse+write (TDD, per
   `xmp.rs`/`xmp_write.rs`'s existing `#[cfg(test)]` module style).
2. Backend integration test: rename a dir-album's directory with a
   `.albuminfo.xmp` present, confirm the new album ID picks up the custom
   title/description/tags/date rather than re-deriving from the path.
3. `just check && just test` green (backend + vitest + Playwright).
4. Manual: edit album title/description/keywords/date via the new "Album
   Info" modal, confirm `.albuminfo.xmp` appears in the directory with
   correct content, and confirm the values survive a manual directory
   rename on disk followed by a re-scan.

## Notes

- 2026-07-01: Design approved by user in brainstorming session on
  `feat/ui-overlays`. Implementing on new branch `feat/album-metadata`
  (worktree `.worktrees/album-metadata`), branched from `feat/ui-overlays`
  since it repurposes `DisplayAlbum.vue` as left by that branch.
