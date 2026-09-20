---
status: open
type: bug
priority: high
area: backend
---

## Notes

Deleting an album should recursively remove sub-albums and files. Instead,
sub-albums get restored to the main directory.

This suggests the delete operation removes the directory's DB record but
not the physical directory, so the next index sweep re-discovers the
sub-albums and re-creates their DB records under the root.

### Progress (2026-09-20)

Reported by user. Needs reproduction and root-cause investigation.
