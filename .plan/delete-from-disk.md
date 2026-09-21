---
status: done
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1a.

## Context

`DELETE /delete/delete-data` removes the DB record and `.xmp` sidecar but never `fs::remove_file` on the original file
or its thumbnail.

## Tasks

- [x] Two-step delete UX: "trash" (soft, existing) → "confirm delete from disk" (hard). TrashedPage exists;
      "Permanently Delete" is wired into the single and batch trashed menus (`SingleMenu.vue`, `BatchMenu.vue`).
- [x] `DELETE /delete/delete-data` (or new endpoint) must: 1. For each alias path, `fs::remove_file` the original 2.
      `fs::remove_file` the `.xmp` sidecar (done) 3. `fs::remove_file` the compressed thumbnail at
      `compressed_path(hash)` 4. Remove from DB (done)
- [x] Handle multi-alias case: only remove from disk when removing the last alias; for earlier aliases only remove that
      alias path from the `alias[]` list.
- [x] `DIR_ALBUM_CACHE` eviction on delete.

## Design (2026-09-08) — SUPERSEDED

The original `aliasList` design was replaced by `asset_ids` in commit `1e8f55f1`. `DELETE /delete/delete-data` now
takes `asset_ids: Vec<String>` — each entry is an asset ID. The backend looks up the asset record, deletes the file
and sidecar, and removes the record. Shared thumbnails are preserved via `DUPE_INDEX`.

## Progress (2026-09-08)

Branch `feat/delete-multi-alias` — TDD red-green implemented.

- `delete_multi_alias.yaml` written: upload identical content → dedup 2-alias record → delete one alias → sibling
  survives, thumbnail served, record locatable.
- `DeleteList` in `backend/src/router/delete.rs` replaced with `asset_ids: Vec<String>` in commit `1e8f55f1`.
- `process_deletes` looks up by `asset_id` in `DATA_TABLE`, deletes file + sidecar, preserves shared thumbnails via
  `DUPE_INDEX`.
- `ItemPermanentlyDelete.vue` sends `assetIds` (not `aliasList`).
- All checks pass: `just check` (clippy, fmt, vue-tsc, eslint, prettier, plan lint), `just test` (258 backend + 33
  Playwright E2E), zero failures.

**Deferred (follow-up PR):** Refactor `start_watcher::handle_removed_file` and `album_index::sweep_stale_aliases` to
reuse shared delete logic (code-sharing requested in design review; adds `.xmp` sidecar cleanup for externally deleted
aliases). The multi-alias "last alias only" rule is no longer applicable — the asset-ID model treats each physical file
as a distinct asset, so deleting one asset never affects same-hash siblings.

PR \#17 review re-check against `main`:

- `process_deletes` (`backend/src/router/delete.rs`) does the disk deletion (originals, `.xmp` sidecars,
  compressed thumbnails).
- Task 2 is implemented on `main`; task 1 (frontend "Permanently Delete" action) also shipped before this PR.
- PR \#17 added the `DIR_ALBUM_CACHE` eviction on album delete (task 4).
- **Resolved: the multi-alias "last alias only" rule (task 3).** The path-primary model treats each file as its own
  asset; deleting one does not affect same-hash siblings. DUPE_INDEX preserves shared thumbnails.
- Test-coverage note: `backend/tests/scenarios/delete_removes_file_and_sidecar_z3.yaml` asserts the original + `.xmp`
  sidecar are gone from disk after `DELETE /delete/delete-data`.
