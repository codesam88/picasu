---
status: open
type: feature
priority: medium
area: backend
---

Tracked in `pre01.md` (meta) as 3a.

## Context

`edit_flags` (`is_favorite`, `is_archived`, `is_trashed`) does not call
`write_sidecar_for`. Other metadata edits (tag, description, rating) do write
sidecars. `is_favorite` has no standard XMP mapping.

## Tasks

- [ ] Call `write_sidecar_for` from `edit_flags`.
- [ ] Decide mapping for `is_favorite`: - `is_favorite=true` → `xmp:Rating=5` - `is_favorite=true` → `xmp:Label="Favorite"` - Or another convention.
