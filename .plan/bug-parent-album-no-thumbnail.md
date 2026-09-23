---
status: open
type: bug
priority: medium
area: backend
---

## Notes

Albums that contain only sub-albums (no direct images) do not get a
thumbnail preview. They should randomly pick an image from one of the
sub-albums as their thumbnail.

The `AlbumCombined::self_update()` in `model/album.rs` computes `cover`
by scanning asset paths whose parent matches the album's `dir_path`. For
parent-only albums this set is empty, so no cover is assigned.

### Progress (2026-09-20)

Reported by user. Needs investigation of `self_update()` cover selection
logic to determine whether sub-album images should be considered.
