---
status: open
type: feature
priority: medium
area: backend
---

Tracked in `pre01.md` (meta) as 3a.

## Context

`edit_flags` (`is_trashed`) does not call
`write_sidecar_for`. Other metadata edits (tag, description, rating) do write
sidecars. `is_favorite` and `is_archived` were removed together with the fields
they toggled, so trash is the only flag left — and it has no standard XMP
mapping.

## Tasks

- [ ] Call `write_sidecar_for` from `edit_flags`.
- [ ] Decide mapping for `is_trashed` (no standard XMP key represents a trash state).
