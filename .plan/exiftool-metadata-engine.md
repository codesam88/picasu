---
status: in-progress
type: feature
priority: high
area: full-stack
---

# ExifTool as the metadata engine (JPEG + PNG metadata coverage)

## Goal

Support all commonly used ways to embed metadata in JPEG and PNG by making
ExifTool the single extraction engine, replacing the hand-rolled byte scan in
`process/xmp.rs` and the EXIF-only `kamadak-exif` reader. No hand-written
metadata parsers: parsing is ExifTool's job, Rust glue is limited to locating
sidecars, invoking the tool, and mapping its JSON onto the app's fields.

## Decisions (settled with the user, 2026-09-27)

1. **Engine: ExifTool** (Phil Harvey, de facto standard). Surveyed
   alternatives: Exiv2 (Rust bindings immature), `exif-oxide` (not production
   ready, AGPL, partial tag coverage), Rust-native fragments (`gamut-*`,
   `xmpkit`, `metastrip` — young, per-scope), `kamadak-exif` (EXIF only, being
   replaced). None comparable as a single established engine.
2. **Integration: safe Rust subprocess, no Perl FFI.** Candidate wrapper is
   the `exiftool` crate (Apache-2.0, `-stay_open` persistent mode); the spike
   in Iteration 1 measures cold `exiftool -j` spawn cost and confirms or
   rejects the crate before it becomes a dependency. ExifTool itself is pure
   Perl and unpacks without root; CI and the Docker image install
   `libimage-exiftool-perl`; dev machines get it through `just install-dev`.
   GPL-as-separate-process matches the existing ffmpeg precedent.
3. **Field mapping.** Fields the app natively models — tags, description,
   rating, title — fill with precedence **XMP > IPTC > PNG-text, first wins**:
   XMP means the sidecar when present, else the embedded packet; IPTC means
   APP13/IIM; PNG-text means `tEXt`/`iTXt`/`zTXt`. Everything else ExifTool
   returns that the app does not model lands in a **further data** bucket:
   simple key/value pairs, displayed in the frontend as a read-only category
   that is not directly manageable or editable.
4. **Scope: JPEG and PNG** (all common carriers: EXIF APP1/`eXIf`, XMP APP1 /
   `iTXt` incl. compact syntax and compression, IPTC IIM APP13, PNG text
   chunks, JPEG comments where ExifTool surfaces them). Video keeps
   ffprobe/ffmpeg — out of scope here.
5. **Replacements:** `kamadak-exif` leaves the backend once no caller remains;
   `process/xmp.rs` keeps sidecar discovery but loses its byte-scan parser and
   its "container-unaware" documentation.
6. **This plan overturns three pins from `test-exif-xmp-handling`** (each flip
   is recorded there when made):
   - PNG embedded XMP was "unsupported, no extraction work" → becomes
     supported (`pinned_png_compressed_embedded_xmp_is_not_extracted` flips to
     a positive test).
   - JPEG IPTC was unclaimed ("backend has no IPTC reader") → becomes
     supported.
   - Sidecar precedence pinned as "corrupt sidecar suppresses _everything the
     file carries_" → becomes "sidecar replaces the XMP source only"; IPTC and
     PNG-text still fill native fields no XMP source supplied. The
     `corrupt_xmp_sidecar_suppresses_embedded_xmp` pin changes meaning and is
     rewritten with the new contract stated in its comments.

## Current state (measured)

- EXIF: `process/exif.rs::generate_exif_for_image` via `kamadak-exif`
  (in-process). `exifVec` values are kamadak display strings —
  `DateTimeOriginal: "2024-05-06 07:08:09"` (dashes),
  `Orientation: "row 0 at right and column 0 at top"`. These observable
  formats change with ExifTool (colon dates, `Rotate 90 CW`-style strings);
  `misc.rs::fix_image_orientation`/`fix_image_width_height` match the kamadak
  strings today and must be rewritten against ExifTool's values, and the
  scenario assertions that pin dash-dates must be updated in the same change.
- XMP: `process/xmp.rs` hand-rolled byte scan (plaintext `<dc:subject>` etc.
  anywhere in the bytes), sidecar preferred via `discover_sidecar`; 20+ unit
  tests including the no-decompression pin and the container-unaware doc.
- IPTC: snapfab _writes_ APP13 IPTC for tagged JPEGs (`iptc` crate); backend
  reads none. PNG text chunks: not read at all. PNG `eXIf`: read via kamadak.
- Manifest `utils/snapfab/capabilities.json`: jpeg `xmp: [embedded,
sidecar]`, `unsupportedMetadataFields: []` (IPTC claim removed in Iteration
  0 of the previous plan); png `xmp: [sidecar]` with `xmp:embedded`
  unsupported. `METADATA_FIELDS = [exif, xmp, container]`,
  `METADATA_SOURCES = [embedded, sidecar, probe]`.
- Tooling: `ffmpeg`/`ffprobe` have a hard precondition test
  (`video_metadata_requires_a_working_ffmpeg_and_ffprobe`); ExifTool needs the
  same. `exiftool` is **not installed** in the dev environment; no root, but
  `~/.local/bin` is on PATH; `just install-dev` exists;
  `.github/workflows/ci.yml` and `./Dockerfile` have no ExifTool step.
- Environment constraints: `.plan/tmpfs-quota-test-runs.md` (TMPDIR
  workaround), `.plan/scenario-harness-debt.md` (Playwright port race).

## Iterative implementation plan

Same contract as the previous plan: worker sub-agents write the minimal
regression test first and demonstrate the failure (or the right-reason pass),
then implement the smallest production change; the parent reviews each diff
and runs focused gates before the next iteration. No unrelated edits, no
silent semantic changes, no independent commits. Product decisions stop the
iteration and are recorded in this plan.

### Iteration 1 — Toolchain + EXIF engine swap

- **Owner:** backend engine worker.
- **Tests first:** ExifTool precondition test mirroring the video one (hard
  failure naming the missing binary and the install remedy, split so the
  message is unit-testable); a mapping test that fails while
  `generate_exif_for_image` still uses kamadak.
- **Spike (record numbers in Progress before implementation):** cold
  `exiftool -j` spawn cost vs the `exiftool` crate `-stay_open` path on this
  machine; exact ExifTool output on our own pinned fixtures (key shapes with
  /without `-G`, date format, `Orientation` print value) — this settles the
  `exifVec` contract this plan adopts.
- **Implementation:** install story first (`just install-dev` fetch of the
  official pure-Perl distribution into a user-writable prefix with a PATH
  symlink — no root; CI workflow step; Dockerfile package; docs line), then
  swap `generate_exif_for_image` to ExifTool JSON, rewrite the orientation and
  dimension-swap matching in `misc.rs` against the measured ExifTool values,
  update the dash-date scenario assertions, remove `kamadak-exif` if no caller
  remains (verify: `exif.rs` was its only user; PNG `eXIf` and TIFF/WebP EXIF
  now come through ExifTool — the tiff/webp/png EXIF scenarios are the proof).
- **Gate:** full `cargo test -p picasu`, `cargo test -p snapfab`, `just
backend-check`, `just utils-check`, `just docs-check`.

### Iteration 2 — Native field mapping with XMP > IPTC > text precedence

- **Owner:** backend mapping worker.
- **Tests first:** pure unit tests over recorded ExifTool JSON payloads (red):
  tags/description/rating/title fill order across XMP-sidecar, XMP-embedded,
  IPTC, and PNG-text sources; IPTC keywords reaching tags; the precedence
  first-wins rule; corrupt/absent sources yielding empty. Then the flipped
  pins: `corrupt_xmp_sidecar_suppresses_embedded_xmp` rewritten for
  "sidecar replaces XMP only" (IPTC may still fill), with the plan reference
  in the comment; every scenario whose tags now come from IPTC asserted
  explicitly.
- **Implementation:** replace `extract_xmp_data*` internals with ExifTool
  invocation (sidecar discovery stays: sidecar exists → ExifTool reads the
  `.xmp`, else the image); keep `XmpData`-shaped output or rename honestly;
  record the three overturned pins from the previous plan in its Progress.
- **Gate:** metadata scenarios, xmp/exif unit tests, `just backend-check`.

### Iteration 3 — Further-data surface (API + frontend)

- **Owner:** full-stack worker.
- **Tests first:** API scenario asserting the new read-only bucket appears in
  `GET /get/metadata/{assetId}` for a fixture carrying IPTC/PNG-text fields
  the app does not model; vitest/Playwright assertion that the sidebar
  renders it as a non-editable category.
- **Implementation:** persisted record field (model + rebuild-safe like
  `exif_vec`), utoipa annotation, `openapi.json` regen (`just openapi-gen`),
  sidebar section (display-only: no edit controls), mapping from ExifTool
  groups (`IPTC:*`, `PNG:*`, unmodelled `XMP:*`, …) into the bucket with the
  documented split from native fields.
- **Gate:** API scenario + targeted frontend suite, `just openapi-docs-check`
  if present, `just check`.

### Iteration 4 — Fixtures, manifest claims, container coverage

- **Owner:** fixture/format worker.
- **Tests first:** PNG fixture carrying compressed `iTXt` XMP + `tEXt`
  fields, and a JPEG IPTC-only (no sidecar, no XMP) tag flow — one scenario
  each; reverse `pinned_png_compressed_embedded_xmp_is_not_extracted` into a
  positive extraction test; manifest tests for the new claims.
- **Implementation:** extend the manifest vocabulary (`METADATA_FIELDS` gains
  `iptc`/`text` with source `embedded`, validation + tests), flip claims:
  jpeg gains `iptc: [embedded]`, png gains `xmp: [embedded]` and drops
  `xmp:embedded` from unsupported; retire `xmp.rs`'s byte-scan parser and its
  container-unaware doc (ExifTool locates containers); fixtures pinned with
  provenance — written _by ExifTool itself_ where possible, so the writer and
  reader are the same established tool.
- **Gate:** manifest tests, full backend suite, snapfab tests.

### Iteration 5 — Docs, cleanup, final gate

- **Owner:** any worker, then parent.
- `docs/design.md` (extraction pipeline), `docs/test-strategy.md` (tool
  precondition), frontend docs if the sidebar section needs a line; confirm
  `kamadak-exif` and the byte-scan are gone from `Cargo.toml`/`Cargo.lock` and
  the tree; release-build compile; `just check` + `just test`; flip this plan
  to `done` with a closing note.

## Sub-agent coordination rules

- One bounded subsystem per worker; exact file ownership in the task prompt.
- Workers report the red test, implementation files, focused verification
  (including mutation checks for new assertions), and unresolved questions.
- The parent reviews each diff before starting the next iteration; the parent
  runs `just plan-lint` and owns `.plan/*` edits.
- A worker may not claim a format supported because a fixture was added — the
  manifest claim, the scenario, and the reader must agree.

## Acceptance criteria

- JPEG: EXIF + XMP (embedded, incl. compact syntax, and sidecar) + IPTC IIM
  read through ExifTool; PNG: EXIF + XMP (embedded incl. compressed `iTXt`,
  and sidecar) + text chunks read through ExifTool; each with at least one
  scenario proving it end-to-end.
- Native fields obey XMP > IPTC > text first-wins; non-modelled fields appear
  in the read-only further-data category in the API and the sidebar.
- `kamadak-exif` and the `xmp.rs` byte-scan parser are gone; no new
  hand-written metadata parsing exists in the tree.
- ExifTool absence fails with an actionable diagnostic (like ffmpeg), on
  every environment: dev (`just install-dev`), CI, Docker image.
- `just check`, `just test`, and the targeted Playwright scenarios pass.

## Progress

- 2026-09-27 — Iteration 1 done (two passes). Second pass adopted the
  `exiftool` crate as decided: one persistent `-stay_open` session per calling
  thread (`thread_local` inside `exif.rs`; the only caller is the rayon index
  pool, so sessions are bounded by pool size), a single grouped read
  (`-G1` + `-d %Y-%m-%d %H:%M:%S`) feeding a projection that keeps the
  pinned `exifVec` contract byte-identical — **zero assertion edits** across
  395 backend tests, with a parity oracle comparing the new path against the
  removed cold invocation on four inputs, plus an ad-hoc run over ExifTool's
  own 194-file corpus: 0 mismatches for JPEG/PNG/TIFF/WebP; 5 raw-format
  divergences, all the same primary-directory choice (IFD0 vs SubIFD),
  documented in `exif_map_from`. Re-measured (release profile, 5 runs): cold
  spawn p50 ~171 ms (baseline reproduced); session per-file p50 8.7–15.0 ms;
  one shared session 62–100 files/s regardless of threads; per-thread sessions
  **330–469 files/s** → ~36 min per 1M images of metadata reads vs 5.9 h for
  the rejected cold spawn. Restart-once on transport failures only
  (`is_transport_failure`), proven by a test that SIGKILLs the live child via
  `/proc/thread-self/children` and asserts the unretried read fails, the
  retried read returns the identical map, and exactly one new child runs;
  mutation: removing the restart fails exactly that test. Missing binary is a
  typed `ExifToolNotFound` surfaced at session creation with the install
  remedy (sessions that fail to start are not cached). Maker-note groups stay
  excluded (parity with the removed `-EXIF:all`). `cargo deny` green
  (`exiftool 0.3.1` Apache-2.0 + `serde_path_to_error`). Gates: `cargo test
-p picasu` 395 (+1 ignored perf harness), `-p snapfab` 62, `just
backend-check/utils-check/docs-check/backend-audit/plan-lint`, targeted
  Playwright 4/4. Open questions for later iterations: **release tarball
  ships without ExifTool** (bundles only `picasu`+ffmpeg; shipping the
  ~500-module Perl tree is a packaging decision — bundle vs document as
  prerequisite, needed before the next tag); whether a swallowed session
  start failure should emit one `log::error!` (currently silent-empty like
  the old kamadak path; per-image spam risk); non-UTF-8 filenames behave as
  before (empty map, different reason).
- 2026-09-27 — Iteration 1 first pass shipped the toolchain (pinned
  `just install-exiftool`, CI + Docker + docs), the precondition test, the
  kamadak removal, and the measured assertion flips — but its execution path
  (cold `exiftool -j` spawn per file, 174 ms p50) was rejected on review: at
  8 index workers that is ~6 h per 1M images of pure process start, and
  Iteration 2's XMP/IPTC read would double it. Spike numbers (50 runs,
  exiftool 13.59): cold spawn p50 **174.4 ms**; raw `-stay_open` protocol p50
  **6.97 ms**; `exiftool` crate 0.3.1 `json()` p50 **12.36 ms** (mean 16.6,
  p95 19.5); perl startup floor 134 ms. Decision after review: **adopt the
  `exiftool` crate** (crate ≈ raw stay_open within noise, and the alternative
  is hand-rolling the protocol), with one persistent session serving a single
  full-metadata call per file (`-G1` groups: `EXIF:`-family keys strip to the
  existing `exifVec` contract, `XMP:`/`IPTC:`/`PNG:` groups are Iteration 2's
  input), and a restart-once-and-retry on typed process errors — the
  empty-map-on-failure contract is unchanged. Accepted trade-offs: a Perl
  child for the server lifetime, serialized access unless instances are
  per-thread, `DateTime`→`ModifyDate` / `DateTimeDigitized`→`CreateDate` /
  `ImageLength`→`ImageHeight` renames visible in the sidebar for old
  libraries, `-d` dash dates kept (sort/parse contract), date-only tags keep
  ExifTool's `YYYY:MM:DD`.
- 2026-09-27 — Plan written. Decisions recorded above from the user session:
  ExifTool over alternatives (survey in decision 1), mapping to a read-only
  further-data bucket, XMP > IPTC > text precedence, kamadak replaced.
