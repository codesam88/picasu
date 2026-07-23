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
| 1a   | Delete removes files from disk     | 🏗️ In progress | `delete-from-disk.md`               |
| 1b   | Watcher handles Remove events      | 🏗️ In progress | `watcher-remove-events.md`          |
| 1c   | assign_album conflict handling     | 🏗️ In progress | `assign-album-conflict.md`          |
| 1d   | Upload conflict handling           | 🏗️ In progress | `upload-conflict.md`                |
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

## Related Plan Items

| File                              | Status      | Notes                                                |
| --------------------------------- | ----------- | ---------------------------------------------------- |
| `delete-from-disk.md`             | in-progress | Delete removes files from disk + thumbnails          |
| `watcher-remove-events.md`        | in-progress | Watcher handles Remove events                        |
| `assign-album-conflict.md`        | in-progress | assign_album conflict handling (rename/replace/skip) |
| `upload-conflict.md`              | in-progress | Upload conflict handling                             |
| `edit-flags-sidecar-writeback.md` | open        | edit_flags writes XMP sidecar                        |
| `frontend-exif-display.md`        | open        | Expand ItemExif.vue beyond Make/Model                |
| `test-exif-xmp-handling.md`       | open        | Non-JPEG container XMP coverage                      |
| `expand-e2e-testing.md`           | open        | 12+ untested API endpoints                           |
| `ui-refinement.md`                | open        | UI bugs and polish                                   |
| `backend-unit-tests.md`           | open        | Pure function unit tests                             |
| `clippy-unwrap-cleanup.md`        | backlog     | ~140 unwrap calls                                    |
| `verify-file-actions.md`          | done        | E2E lifecycle coverage                               |
| `stale-dir-album-cache.md`        | done        | Cache pruned at startup + request time               |
| `frontend-rating-widget.md`       | done        | Star rating UI + edit_rating endpoint                |
| `xmp-sidecar-metadata.md`         | done        | XMP sidecar lifecycle                                |
