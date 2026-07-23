---
status: in-progress
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1d.

## Context

`post_upload` renames from tmp to final path with no conflict check —
`fs::rename` overwrites silently if the file already exists.

## Tasks

- [ ] Same `on_conflict` parameter pattern as `assign-album-conflict`:
      `"rename" | "replace" | "skip"` (default: `"skip"`).
