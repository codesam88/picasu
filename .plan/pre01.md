---
status: in-progress
type: meta
priority: high
area: meta
---

# pre01: v0.1 Release Plan

## What this release promises

1. **File operations stay in sync** — delete, move, upload, and external changes keep the
   filesystem and database consistent (originals, thumbnails, sidecars, caches).
2. **Metadata is read from files** — EXIF/XMP extracted at index time and shown in the UI.
3. **User edits are written back** — editing metadata never touches the original asset;
   changes go to an XMP sidecar next to the file (`{name}.{ext}.xmp`, or `.albuminfo.xmp`
   for albums).

**Release rule:** anything editable in the UI must be a working, confirmed function.

## Workstreams

The remaining release work falls into four categories:

| Category                | What it covers                                      | Items      |
| ----------------------- | --------------------------------------------------- | ---------- |
| **A. Correctness**      | Behavior that is wrong or violates the release rule | —          |
| **B. Metadata & UI**    | Completing the metadata promise; visible polish     | B1, B2, B3 |
| **C. Release hygiene**  | Legal/licensing gate before tagging                 | C1         |
| **D. Confidence tests** | Tests that pin down shipped behavior                | D1         |

## Status overview

| #   | Item                                    | Ticket                                   | Status                                        |
| --- | --------------------------------------- | ---------------------------------------- | --------------------------------------------- |
| A1  | Delete album resurrects sub-albums      | `bug-delete-album-restores-subalbums.md` | done — fix + e2e coverage on main             |
| A2  | Flag edits don't write sidecars         | `bug-trash-sidecar-durability.md`        | done — trash marker durable in sidecar        |
| B1  | EXIF/XMP read for non-JPEG containers   | — (ticket removed)                       | done — per-format scenarios on main           |
| B2  | UI bugs (Escape/back, lightbox, theme)  | `ui-refinement.md`                       | open — bug checklist only; 4/21 done          |
| B3  | Parent-only albums have no thumbnail    | `bug-parent-album-no-thumbnail.md`       | done — descendant cover fallback              |
| C1  | License / SPDX / OSSF review            | `license-and-ossf-review.md`             | idea — promote to open, run before tagging    |
| D1  | Filename sanitization corner-case tests | `filename-sanitization-test-gaps.md`     | done — homoglyphs stripped, all 5 gaps closed |

## Item details (what / why)

### A. Correctness

**A1 — Deleting an album resurrects its sub-albums.**
_Fixed 2026-09-25_ (`43365fc4`, closed 2026-10-09). `delete_data` now runs
`cleanup_album_descendants` before dropping rows: descendant files/sidecars
removed, descendant rows removed (album records carry their directory path, so
sub-albums match `get_assets_under_path`), then `remove_dir_all` on the album
directory — nothing is left for the sweep to re-discover. Guarded by
`delete-parent-album-does-not-restore-child.yaml`,
`album_delete_recursive.yaml`, `album_delete_parent_and_child_together.yaml`
and three more, running in CI via `just test`. Residual gap recorded in the
ticket: `remove_dir_all` failures are swallowed, so an fs-level failure would
still produce this symptom silently.

**A2 — Trashed doesn't write an XMP sidecar.**
_Fixed 2026-10-09_ (`bug-trash-sidecar-durability.md`). `PUT /put/edit_flags`
now routes through `commit_metadata_edits`: the trash flag is applied to the
composed view, written to the sidecar as the managed `picasu:Trashed`
property, and only then stored on the record — the same write-then-store
contract `edit_tag`/`edit_rating` follow. Restore deletes the property. The
flag is durable in the sidecar, so a `POST /post/rebuild` (or losing
`DATA_HOME`) keeps trashed items trashed instead of silently undeleting them;
`process/index.rs`, `dir_album.rs` and `rebuild.rs` compose it back at index
time. Pinned by `trash_state_survives_rebuild.yaml` and the xmp read/write
module tests; backend suite 458 green. _Why it was critical:_ flag editing is
exposed in the UI (delete / restore menu items), and DB-only trash state
violated the design doc's "generated state is rebuildable from the
filesystem" rule — the release rule's correctness half.

### B. Metadata & UI

**B1 — Non-JPEG XMP read coverage.**
_Done._ Main now carries real-fixture metadata scenarios for PNG, TIFF, WebP, MP4 and MOV
(`png_metadata_exif_dimensions_thumbnail`, `tiff_…`, `webp_…`, `mp4_ffprobe_…`,
`mov_ffprobe_…`), each paired with a `*_without_xmp_source_has_no_tags` negative, plus
corrupt EXIF/XMP cases and seeded randomized format runs driven by
`utils/snapfab/capabilities.json`. Extraction is `extract_xmp_data_from_packet` /
`extract_xmp_data_from_file` (`backend/src/process/xmp.rs`), no longer a substring scan;
`ffmpeg`/`ffprobe` absence is pinned by
`video_metadata_requires_a_working_ffmpeg_and_ffprobe`. _Residual:_ video extensions
outside the matrix (gif, webm, mkv, avi, flv, wmv, mpeg) have unit-level detection coverage
only; IPTC-IIM and MP4 UUID-box XMP are unclaimed; PNG embedded XMP is pinned as not
extracted. Ticket file removed 2026-10-08.

**B2 — UI bug pass.**
_What:_ the bug checklist in `ui-refinement.md` — Escape/back navigation, lightbox
controls, theme placement. _Why:_ daily-visible glitches that undermine the first
impression. The polish/feature items in the same ticket (breadcrumbs, nav rework) are
**not** release-scoped.

**B3 — Parent-only albums show no thumbnail.**
_Done 2026-09-25._ `AlbumCombined::self_update()` computed the cover from assets whose
parent matches `dir_path`, an empty set for parent-only albums; the fix picks the newest
eligible descendant image instead, ignoring generated `.__picasu_ph__.jpg` placeholders.
Unit coverage plus an API scenario verifying the parent album exposes the descendant
asset as its cover.

### C. Release hygiene

**C1 — License / SPDX / OSSF scorecard review.**
_What:_ review dependencies and repo metadata for license issues; add SPDX labels; check
OSSF scorecard. _Why:_ explicit pre-first-release gate; currently an `idea` — promote to
`open` when starting.

### D. Confidence tests

**D1 — Filename sanitization corner-case tests.**
_Fixed 2026-10-09_ (`c537652f` plus the NFC-off follow-up, closed 2026-10-09). Control characters (C0/DEL/C1) are now
stripped by an always-on tier; separator homoglyphs (`／＼｜` and mathematical look-alikes) are stripped by tier 2.
`resolve_filename` has direct unit tests, tier-2 boundaries are pinned, and NFC-on/off upload scenarios use YAML
`\u` escapes to carry genuine NFD filenames. See `filename-sanitization-test-gaps.md` for the full record.

## Explicitly out of scope for v0.1

- **Image title editing** — not exposed in the UI; nothing to make work.
- **Tag origin tracking** ("provenance") — user tag edits already persist to sidecars;
  origin tracking only matters for a future scrub/repair feature.
- **`error-handling-and-activity-log`** — largest open ticket, cross-cutting scope;
  deferring does not violate the release rule. Post-v01 (or a backend-logging-only slice).
- **`openapi-contract-hardening`** — only the two high items (renew-hash-token
  registration, 401 sweep) are candidates if schedule allows; the remaining ~11 tasks
  backlog.
- **Test infrastructure** — coverage reports, Pinia store tests, e2e expansion (closed
  obsolete 2026-09-24), unified harness, quality gates.
- **Backlog architecture/perf work** — fs-db review, clippy unwrap cleanup, view-cache
  eviction, scrub endpoint (stale design), docker verification (unless Docker is a
  release deliverable).

## Already done (for context)

File lifecycle (delete removes files, watcher Remove handling, `assign_album` conflicts,
upload conflict handling + hardening), EXIF display in the metadata panel, rating field,
sidecar lifecycle (moves/deletes with file), dir-album cache pruning, backend unit tests
for the five pure-function targets, snapfab migration.

## Related plan items

| File                                     | Status | Item |
| ---------------------------------------- | ------ | ---- |
| `bug-delete-album-restores-subalbums.md` | done   | A1   |
| `bug-trash-sidecar-durability.md`        | done   | A2   |
| `ui-refinement.md`                       | open   | B2   |
| `bug-parent-album-no-thumbnail.md`       | done   | B3   |
| `license-and-ossf-review.md`             | idea   | C1   |
| `filename-sanitization-test-gaps.md`     | done   | D1   |
