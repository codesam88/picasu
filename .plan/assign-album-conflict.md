---
status: in-progress
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1c.

## Context

`assign_album` renames the file to `album_dir/filename` and renames the `.xmp`
sidecar alongside it. On conflict, `fs::rename` silently overwrites. No
`on_conflict` parameter exists.

## Tasks

- [ ] Add `on_conflict: "rename" | "replace" | "skip"` to `AssignAlbumData`.
      Default: `"skip"` so current callers don't silently overwrite.
- [ ] Before `fs::rename`: check if `dest_path` exists. If so: - `"skip"`: return early (or 409) - `"replace"`: only if hashes match, else error - `"rename"`: append `_1`, `_2`, ... until a free name is found
- [ ] Sidecar is already handled — ensure it uses the resolved destination.
