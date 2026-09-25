---
status: done
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

## Resolution

The favorite and archived flags were removed. The remaining trash flag is an
application lifecycle state, not durable file metadata; no XMP mapping was
selected. This item is closed without adding a sidecar write from
`edit_flags`.
