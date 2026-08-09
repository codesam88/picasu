---
status: in-progress
type: meta
priority: high
area: meta
---

# pre01: First-Release Feature Plan

## Overview

Three interlocking areas for first release: file lifecycle correctness, metadata
read from files, and metadata write-back to XMP sidecars. A fourth cross-cutting
concern — rating — falls out of the metadata work.

## Tickets

| Area | Summary                            | Status         | Ticket                              |
| ---- | ---------------------------------- | -------------- | ----------------------------------- |
| 1a   | Filesystem-based trash + delete    | ✅ Done        | `delete-from-disk.md`               |
| 1b   | Watcher handles Remove events      | 🏗️ In progress | `watcher-remove-events.md`          |
| 1c   | assign_album conflict handling     | 🏗️ In progress | `assign-album-conflict.md`          |
| 1d   | Upload conflict handling           | ✅ Done        | `upload-conflict.md`                |
| 1e   | Verify file E2E                    | ✅ Done        | `verify-file-actions.md`            |
| 2a   | XMP metadata read (IPTC, GPS)      | 🟡 Partial     | `test-exif-xmp-handling.md`         |
| 2b   | Frontend EXIF display              | ❌ Open        | `frontend-exif-display.md`          |
| 3a   | XMP sidecar write-back (all edits) | 🟡 Partial     | `edit-flags-sidecar-writeback.md`   |
| 3b   | Sidecar moves with file            | ✅ Done        | `xmp-sidecar-metadata.md`           |
| 3c   | Sidecar deletes with file          | ✅ Done        | `xmp-sidecar-metadata.md`           |
| 3d   | Tag provenance                     | 🟡 Minimal     | no dedicated ticket (v1 acceptable) |
| 4a   | Rating field                       | ✅ Done        | `frontend-rating-widget.md`         |
| 4b   | Image title field                  | ⏭️ Deferred    | no dedicated ticket                 |
| 5    | DIR_ALBUM_CACHE stale entries      | ✅ Done        | `stale-dir-album-cache.md`          |

## Current Next Steps

The release-critical work is now concentrated in filesystem lifecycle behavior
and the remaining metadata paths. Upload handling is complete, including conflict
strategies, filename sanitization, content validation, timestamp handling, bounded
auto-rename, multi-file preflight validation, and frontend upload-option coverage.

### Next, in order

1. Complete filesystem-based trash + delete lifecycle: two-stage delete,
   restore-as-assign-album, album cascade, multi-alias handling, config, filter
   changes, frontend wiring (`delete-from-disk.md`). Spec:
   `docs/superpowers/specs/2026-08-08-delete-lifecycle-design.md` and
   `docs/superpowers/specs/2026-08-09-restore-as-assign-album-design.md`.
2. Handle externally deleted files in the watcher and during manual album indexing
   (`watcher-remove-events.md`). Note: watcher now also ignores `.trash/` prefix
   (covered by delete lifecycle).
3. Add conflict handling to `assign_album`, including resolved sidecar destinations
   (`assign-album-conflict.md`).
4. Complete XMP sidecar write-back for `edit_flags`, after deciding the favorite
   mapping (`edit-flags-sidecar-writeback.md`).
5. Add non-JPEG XMP/IPTC coverage and expand frontend EXIF display
   (`test-exif-xmp-handling.md`, `frontend-exif-display.md`).
6. Run the remaining release-confidence work: API/UI E2E coverage, backend unit
   coverage, and the highest-impact UI bugs (`expand-e2e-testing.md`,
   `backend-unit-tests.md`, `ui-refinement.md`).

Image title editing remains explicitly deferred for v0.1.

## Related Plan Items

| File                              | Status      | Notes                                                  |
| --------------------------------- | ----------- | ------------------------------------------------------ |
| `delete-from-disk.md`             | in-progress | Filesystem-based trash, untrash, album cascade, config |
| `watcher-remove-events.md`        | in-progress | Watcher handles Remove events                          |
| `assign-album-conflict.md`        | in-progress | assign_album conflict handling (rename/replace/skip)   |
| `upload-conflict.md`              | done        | Upload conflict handling and upload hardening          |
| `edit-flags-sidecar-writeback.md` | open        | edit_flags writes XMP sidecar                          |
| `frontend-exif-display.md`        | open        | Expand ItemExif.vue beyond Make/Model                  |
| `test-exif-xmp-handling.md`       | open        | Non-JPEG container XMP coverage                        |
| `expand-e2e-testing.md`           | open        | 12+ untested API endpoints                             |
| `ui-refinement.md`                | open        | UI bugs and polish                                     |
| `backend-unit-tests.md`           | open        | Pure function unit tests                               |
| `clippy-unwrap-cleanup.md`        | backlog     | ~140 unwrap calls                                      |
| `verify-file-actions.md`          | done        | E2E lifecycle coverage                                 |
| `stale-dir-album-cache.md`        | done        | Cache pruned at startup + request time                 |
| `frontend-rating-widget.md`       | done        | Star rating UI + edit_rating endpoint                  |
| `xmp-sidecar-metadata.md`         | done        | XMP sidecar lifecycle                                  |
