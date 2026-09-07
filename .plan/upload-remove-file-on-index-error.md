---
status: open
type: bug
priority: medium
area: backend
---

Failed-upload cleanup removes the file on **any** `index_image` error. Raised in PR \#17 review (2026-09-07), needs
confirming and deciding.

## Context

`post_upload.rs:254-266`: if `crate::workflow::index_image(relative_src, None)` returns `Err`, the just-written file is
removed and a 400 is returned. The doc comment justifies this only for a "never-indexed" file, but
`workflow::index_image` (`workflow/mod.rs:78`) returns `anyhow::Error` from every stage — `OpenFileTask`, `HashTask`,
`DeduplicateTask`, `IndexTask`, `VideoTask` — including `ErrorKind::Internal` (coordinator join, DB) failures that can
occur **after** a partial commit.

Risk: an internal failure mid-`IndexTask`/`VideoTask` could both delete the user's file and leave a DB record whose
alias now points at a missing path (broken alias), i.e. data loss plus a stale record. Only a content-decode failure
guarantees nothing was committed.

## Tasks / decision

- [ ] Confirm whether any post-commit error path exists in `index_image` (partial commit before an error) — audit
      `IndexTask`/`VideoTask`.
- [ ] Decide: restrict file removal to content-decode failures, or verify the record is absent (check
      `DATA_TABLE`/in-memory tree for `hash`) before removing for other error kinds.
- [ ] Scenario: current `upload_unindexable_removed` covers only the decode-fail path. Add a scenario asserting no file
      removal on an internal-error path (if deterministically reachable).

## Notes

Deletion boundary agreed 2026-08-08 (recorded in `upload-conflict.md`): removal is only legal within the failed upload
request; never after a successful upload. This ticket is about narrowing _which_ failures are "never succeeded".
