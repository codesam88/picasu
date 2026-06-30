---
status: done
type: feature
priority: medium
area: backend
---

Tags assigned via API should be merged into DB as well as image repository on disk (idea borrowed from Immich):

1. Use XMP sidecar files to store all extracted data without modifying originals
2. On indexing, if no XMP sidecar exists, create from image metadata
3. On tag or other metadata changes, first store to db and then to XMP sidecar
4. On album assignment (causing the original file to be moved), XMP sidecar files are moved together with the originals
5. Provide a separate helper script to merge metadata from sidecar files back into originals

DEFERRED — not part of the storage-architecture fix; XMP sidecars are a separate future step.

2026-06-30: Done. Full XMP sidecar lifecycle implemented on pre01 branch:

- `xmp.rs`: byte-scan parser for dc:subject, dc:description, xmp:Rating; sidecar-first discovery at index time
- `xmp_write.rs`: atomic sidecar write (temp+rename) wired into edit_tag, edit_description, edit_rating
- `assign_album`: sidecar renamed alongside original
- `delete_data`: sidecar deleted alongside original
