---
status: in-progress
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

### Progress (2026-09-25)

Expanded API coverage for recursive deletion and added a frontend Playwright
scenario for the reported flow: soft-delete a parent album, permanently delete
it from Trash, then verify the child does not return. The scenario passes, so
the UI flow does not reproduce the resurrection bug on the current tree.

The normal album Delete action is a soft delete; recursive filesystem cleanup
only occurs through the permanent-delete action.

### Progress (2026-09-20)

Reported by user. Needs reproduction and root-cause investigation.
