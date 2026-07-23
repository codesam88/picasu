---
status: in-progress
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1a.

## Context

`DELETE /delete/delete-data` removes the DB record and `.xmp` sidecar but never
`fs::remove_file` on the original file or its thumbnail.

## Tasks

- [ ] Two-step delete UX: "trash" (soft, existing) → "confirm delete from disk"
      (hard, missing). TrashedPage exists; needs "permanently delete" action.
- [ ] `DELETE /delete/delete-data` (or new endpoint) must: 1. For each alias path, `fs::remove_file` the original 2. `fs::remove_file` the `.xmp` sidecar (done) 3. `fs::remove_file` the compressed thumbnail at `compressed_path(hash)` 4. Remove from DB (done)
- [ ] Handle multi-alias case: only remove from disk when removing the last alias;
      for earlier aliases only remove that alias path from the `alias[]` list.
- [ ] `DIR_ALBUM_CACHE` eviction on delete.
