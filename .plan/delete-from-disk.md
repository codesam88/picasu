---
status: in-progress
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
- [ ] Handle multi-alias case: only remove from disk when removing the last alias; for earlier aliases only remove that
      alias path from the `alias[]` list.
- [x] `DIR_ALBUM_CACHE` eviction on delete.

## Progress (2026-09-07)

PR \#17 review re-check against `main`:

- `process_deletes` (`backend/src/router/delete.rs:117-138`) already does the disk deletion (originals, `.xmp` sidecars,
  compressed thumbnails) — the plan's original context ("never fs::remove\_file") was stale.
- Task 2 is therefore already implemented on `main`; task 1 (frontend "Permanently Delete" action) also shipped before
  this PR — this PR only reorganized the menus and added E2E coverage.
- PR \#17 added the `DIR_ALBUM_CACHE` eviction on album delete (task 4).
- **Remaining open item: the multi-alias "last alias only" rule (task 3).** Current behavior removes every alias path
  and drops the whole record regardless of how many aliases a hash has; the intended rule keeps the record (minus the
  removed alias) when other aliases still exist.
- Test-coverage note: `backend/tests/scenarios/delete_removes_file_and_sidecar_z3.yaml` asserts the original + `.xmp`
  sidecar are gone from disk after `DELETE /delete/delete-data`, but **nothing asserts the compressed thumbnail removal**
  — worth extending that scenario (or adding a `file_absent` on the thumbnail path) when task 3 is tackled.
