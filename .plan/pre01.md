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
| **A. Correctness**      | Behavior that is wrong or violates the release rule | A1, A2     |
| **B. Metadata & UI**    | Completing the metadata promise; visible polish     | B1, B2, B3 |
| **C. Release hygiene**  | Legal/licensing gate before tagging                 | C1         |
| **D. Confidence tests** | Tests that pin down shipped behavior                | D1         |

## Status overview

| #   | Item                                    | Ticket                                   | Status                                     |
| --- | --------------------------------------- | ---------------------------------------- | ------------------------------------------ |
| A1  | Delete album resurrects sub-albums      | `bug-delete-album-restores-subalbums.md` | open — needs repro + root cause            |
| A2  | Flag edits don't write sidecars         | `edit-flags-sidecar-writeback.md`        | open — needs trash XMP mapping decision    |
| B1  | EXIF/XMP read for non-JPEG containers   | `test-exif-xmp-handling.md`              | open — JPEG covered; PNG/TIFF/MP4 gap      |
| B2  | UI bugs (Escape/back, lightbox, theme)  | `ui-refinement.md`                       | open — bug checklist only; 4/21 done       |
| B3  | Parent-only albums have no thumbnail    | `bug-parent-album-no-thumbnail.md`       | done — descendant cover fallback           |
| C1  | License / SPDX / OSSF review            | `license-and-ossf-review.md`             | idea — promote to open, run before tagging |
| D1  | Filename sanitization corner-case tests | `filename-sanitization-test-gaps.md`     | open — behavior decisions + tests          |

## Item details (what / why)

### A. Correctness

**A1 — Deleting an album resurrects its sub-albums.**
_What:_ deleting an album should recursively remove sub-albums and files; instead the
physical directory survives, the next index sweep re-discovers the sub-albums, and they
reappear under the root. _Why critical:_ reported by a user; directly contradicts the
file-lifecycle work already shipped (delete-from-disk, watcher Remove handling). Needs
reproduction and root-cause investigation before estimate is reliable.

**A2 — Trashed doesn't write an XMP sidecar.**
_What:_ `PUT /put/edit_flags` updates the database but never calls `write_sidecar_for`,
unlike tag, description, rating, and album-title edits. Decide how `is_trashed` maps to
XMP, then call the writer. _Why critical:_ flag editing is exposed in the UI (delete /
restore menu items), so this is the one editable surface that fails the release rule.
Small scope: one call site + one mapping decision. (Favorite and archived were removed
with the branch that dropped those fields, so only trash remains to map.)

### B. Metadata & UI

**B1 — Non-JPEG XMP read coverage.**
_What:_ XMP extraction (`xmp.rs`) is unit-tested only against JPEG-style packets; XMP/IPTC
packet placement differs per container (PNG zTXt/iTXt, TIFF, MP4 uuid box). Add one test
per representative container. _Why:_ "metadata read from files" is a core release promise;
today it is only demonstrably true for JPEG. Video-pipeline coverage needs
`ffmpeg`/`ffprobe` and may be split off if unavailable in CI.

**B2 — UI bug pass.**
_What:_ the bug checklist in `ui-refinement.md` — Escape/back navigation, lightbox
controls, theme placement. _Why:_ daily-visible glitches that undermine the first
impression. The polish/feature items in the same ticket (breadcrumbs, nav rework) are
**not** release-scoped.

**B3 — Parent-only albums show no thumbnail.**
_What:_ albums containing only sub-albums get no cover; `AlbumCombined::self_update()`
computes the cover from assets whose parent matches `dir_path`, and that set is empty for
parent-only albums. Pick a descendant image instead. _Why:_ visible gap in every album
tree; root cause is already identified, fix is local.

### C. Release hygiene

**C1 — License / SPDX / OSSF scorecard review.**
_What:_ review dependencies and repo metadata for license issues; add SPDX labels; check
OSSF scorecard. _Why:_ explicit pre-first-release gate; currently an `idea` — promote to
`open` when starting.

### D. Confidence tests

**D1 — Filename sanitization corner-case tests.**
_What:_ pin down `sanitize_filename` / `resolve_filename` corner cases raised in PR #17
review: C0/DEL control characters (currently unfiltered — decide policy first), `resolve_filename`
unit tests (generated-name fallback, `file_stem` edges, reject-message paths), fullwidth
separators, tier-2 boundary coverage. _Why:_ filename handling is upload-path security
surface with shipped behavior that has known untested branches; tests are cheap and the
one policy decision (control chars) is small.

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
| `bug-delete-album-restores-subalbums.md` | open   | A1   |
| `edit-flags-sidecar-writeback.md`        | open   | A2   |
| `test-exif-xmp-handling.md`              | open   | B1   |
| `ui-refinement.md`                       | open   | B2   |
| `bug-parent-album-no-thumbnail.md`       | open   | B3   |
| `license-and-ossf-review.md`             | idea   | C1   |
| `filename-sanitization-test-gaps.md`     | open   | D1   |
