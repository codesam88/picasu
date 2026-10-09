---
status: done
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

### Verification (2026-10-09)

Not reproducible on the current tree; closing. Evidence:

- `delete_data` (`backend/src/router/delete.rs`) runs
  `cleanup_album_descendants` before dropping rows: it deletes descendant
  files/sidecars, removes descendant asset rows (album records carry their
  directory path, so sub-albums match `get_assets_under_path`), and calls
  `remove_dir_all` on the album directory. With the directory gone, the
  sweep has nothing to re-discover. Added by 43365fc4
  ("path-primary: recursive album deletion + descendant cleanup", 2026-09-25).
- Only the Trash "Permanently Delete" action reaches `/delete/delete-data`;
  the regular Delete action is a soft delete (trash flag) by design.
- Regression guarded by e2e scenarios (`frontend/tests/playwright/scenarios/`):
  `delete-parent-album-does-not-restore-child.yaml`,
  `album_delete_recursive.yaml`, `album_delete_parent_and_child_together.yaml`,
  plus three more corner cases. These run in CI via `just test`
  (`frontend-playwright` → `interpreter.spec.ts`); main is green as of
  2026-10-08 (job `checks-n-tests`).

Residual gap, not the reported bug: `remove_dir_all` failures are swallowed
(`let _ =`) and descendant file deletions only warn. If fs removal fails
(permissions, locks), rows are dropped while the directory survives, which
would re-trigger this exact symptom silently. No test covers that failure
mode.

### Progress (2026-09-25)

Expanded API coverage for recursive deletion and added a frontend Playwright
scenario for the reported flow: soft-delete a parent album, permanently delete
it from Trash, then verify the child does not return. The scenario passes, so
the UI flow does not reproduce the resurrection bug on the current tree.

The normal album Delete action is a soft delete; recursive filesystem cleanup
only occurs through the permanent-delete action.

### Progress (2026-09-20)

Reported by user. Needs reproduction and root-cause investigation.
