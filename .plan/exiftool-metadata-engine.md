---
status: done
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
7. **ExifTool's write capability is the scope boundary; the parser is not
   ours to test** (user, 2026-09-27). snapfab writes fixture metadata through
   ExifTool itself (writer = reader, round-trip by construction), and any
   feature ExifTool cannot write — measured, e.g. compressed PNG `iTXt`,
   possibly attribute-form XMP — is _not claimed and not tested_: out of
   scope, not an open gap. What this plan tests is the integration and the
   overrides (mapping, precedence, sidecar rules, the further-bucket split),
   not ExifTool's parsing fidelity. The acceptance criterion's "incl. compact
   syntax" is therefore conditional on the Iteration 6 measurement: covered
   if ExifTool writes compact form, amended out of the criterion if it does
   not.

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

### Iteration 6 — snapfab writes metadata through ExifTool (decision 7)

- **Owner:** fixture worker.
- **Measurement first (the report's opening table):** what ExifTool 13.59
  _writes_ and our engine reads back — JPEG APP1 XMP (element form),
  attribute-form/compact XMP (the open question), IPTC IIM, EXIF, PNG
  `eXIf`, PNG `iTXt`/`tEXt`/compressed `iTXt` (the last two known-negative:
  reconfirm). Every "no" becomes an out-of-scope line per decision 7, and
  `test-exif-xmp-handling`'s compact-syntax acceptance clause gets amended by
  the parent if compact is unwritable.
- **Tests first:** a snapfab precondition test for `exiftool` on `PATH`
  (hard-failure diagnostic, same pattern as the backend's); red/parity tests
  showing the current writers' observable outputs (dates, tags, IPTC
  datasets, PNG `eXIf`, XMP APP1) are reproduced through the ExifTool path.
- **Implementation:** `PhotoSpec`'s surface (`tags`, `exif_date`,
  `further_iptc`, `format`, dimensions, `minimal`) stays; the internal
  writers move to one shared ExifTool session (the `exiftool` crate, same
  pattern as `process::exif`). Retire only what the new path makes dead:
  `little_exif` (and the hand-spliced PNG `eXIf` + CRC table), the `iptc`
  crate (+ pad-byte fix), `build_xmp_app1`/`splice_segment` — each deletion
  gated on the full suites staying green, not on intent. Pinned fixtures are
  _never_ regenerated (their SHAs are policy); test-time generation may
  change bytes freely as long as semantic assertions hold. Playwright's
  `snapfab batch` path uses the same code — CI and dev both have ExifTool.
- **Gate:** `cargo test -p snapfab` + `cargo test -p picasu` (full),
  `just check`, full `just test`, `cargo deny`; writer-retirement diff
  reviewed by the parent before this plan closes.

## Sub-agent coordination rules

- One bounded subsystem per worker; exact file ownership in the task prompt.
- Workers report the red test, implementation files, focused verification
  (including mutation checks for new assertions), and unresolved questions.
- The parent reviews each diff before starting the next iteration; the parent
  runs `just plan-lint` and owns `.plan/*` edits.
- A worker may not claim a format supported because a fixture was added — the
  manifest claim, the scenario, and the reader must agree.

## Acceptance criteria

- JPEG: EXIF + XMP (embedded, and sidecar) + IPTC IIM
  read through ExifTool; PNG: EXIF + XMP (embedded incl. compressed `iTXt`,
  and sidecar) + text chunks read through ExifTool; each with at least one
  scenario proving it end-to-end. **Compact/attribute-form XMP amended out**
  by decision 7: ExifTool cannot _write_ the attribute form (it always
  re-serializes element form — measured), the parser is not ours to prove, and
  the integration that is ours to prove is covered by the element-form
  scenarios. The already-pinned compressed-`iTXt` PNG fixture stays: its
  coverage landed before decision 7 and proves _our_ reader's integration with
  a real-world encoding, which is exactly the integration-and-overrides goal.
- Native fields obey XMP > IPTC > text first-wins; non-modelled fields appear
  in the read-only further-data category in the API and the sidebar.
- `kamadak-exif` and the `xmp.rs` byte-scan parser are gone; no new
  hand-written metadata parsing exists in the tree.
- ExifTool absence fails with an actionable diagnostic (like ffmpeg), on
  every environment: dev (`just install-dev`), CI, Docker image.
- `just check`, `just test`, and the targeted Playwright scenarios pass.

## Progress

- 2026-09-28 — **Decision 6 amended** (by the "Sidecar Edits Must Override
  Embedded Metadata" follow-up in `.plan/test-exif-xmp-handling.md`, which the
  user requested after review found edits being resurrected by embedded IPTC on
  reindex). Old rule: "a sidecar replaces the XMP source only; IPTC and
  PNG-text still fill native fields no XMP source supplied." New rule: a
  sidecar's **existence** selects the regime — with a sidecar, all managed
  fields (tags, description, rating, title) come from the sidecar alone
  (absent ⇒ empty, present-blank authoritative); file families (IPTC, PNG-text,
  embedded XMP) fill them only when no sidecar exists. The Iteration-2 pins
  that encoded the old rule (`corrupt sidecar still lets IPTC fill`,
  `sidecar replaces XMP source and image IPTC still fills`, blank fall-through
  for sidecar values) were flipped with the amendment cited in their comments;
  scenario filenames keep their stems because this file's progress notes
  reference them. Accepted consequences: an external partial sidecar
  suppresses the file's tags; a corrupt sidecar withholds every managed field
  until the next app edit rewrites it.
- 2026-09-28 — Iteration 6 done, plan closed. snapfab now writes fixture
  metadata through ExifTool (decision 7): `PhotoSpec`'s surface is unchanged,
  the internals map to one `-stay_open` call per file over a `thread_local`
  session (a process-global `LazyLock` was measured and rejected — statics
  never drop, leaking one Perl child per invocation). Guards cover ExifTool's
  silent-failure shapes: empty/newline values refused up front, and
  `assert_wrote` panics unless ExifTool reports exactly one image updated.
  Writers retired: `little_exif` (incl. the hand-spliced PNG `eXIf` + CRC
  table), the `iptc` crate (+ pad-byte fix), `build_xmp_app1`/`splice_segment`
  — 13 packages left `Cargo.lock`, `cargo deny` warning-free after the stale
  `quick-xml` ignores were dropped. **Measurement corrections to the plan:**
  PNG `tEXt` _is_ writable (the Iteration 6 section's known-negative list was
  wrong; `-PNG:Comment=` etc. produce `tEXt`, switching to `iTXt` only for
  non-Latin-1); attribute-form XMP is produced by neither the tag API nor any
  flag, though a raw `-XMP<=file` import passes bytes through verbatim — and
  the next XMP tag-write re-serializes to element form, so it is a
  pass-through, not a write snapfab has a field for (decision 7 holds, with
  that precision); compressed PNG `iTXt` negative reconfirmed
  (`-compress`/`-Compressed`/`TextChunkType` all rejected or inert). Review
  round: the swap initially left 4 backend tests + 1 Playwright assertion red
  in files outside the worker's ownership — fixed as requested with the
  contract made writer-agnostic rather than re-pinned: `exif.rs` discovers
  whichever byte-order word the file carries (with a new load-bearing
  companion test proving the corruption empties the map while IFD-offset
  damage recovers), the corrupt-EXIF scenario re-hexed to `MM\0*` with a
  measured red→green and an offset-damage mutation, and the two writer stamps
  ExifTool adds (`IPTC:ApplicationRecordVersion`, `XMP-x:XMPToolkit`) are now
  _expected_ in the further bucket — keys asserted, version strings
  deliberately not (13.50 vs 13.59). Docs swept (`metadata.md` writer table,
  `test-strategy.md` write precondition, `paste-shim/README.md`). Left as
  debt: the `snapfab` CLI samples `manifest.formats` instead of the selector
  and panics on pinned draws — filed in `.plan/scenario-harness-debt.md`.
  Gates: `cargo test -p picasu` 419 + 3 integration (0 failed, 1 ignored),
  `cargo test -p snapfab` 75, `just check`, full `just test` (vitest 75,
  Playwright 40), `cargo deny check` — all pass; pinned fixtures byte-identical
  (digest tests green).
- 2026-09-28 — **Plan closed, `status: done`.** Final acceptance check:
  JPEG EXIF/XMP/IPTC and PNG EXIF/embedded-XMP/text each proven end-to-end by
  a scenario (compact/attribute syntax amended out per decision 7, measured
  above); native precedence and the read-only further bucket live in API +
  sidebar; `kamadak-exif`, the byte-scan parser, and snapfab's hand-rolled
  writers are all gone from the tree; ExifTool absence fails with an
  actionable diagnostic in dev/CI/Docker (and is logged at error level in
  production reads); `just check` and `just test` pass. Release tarballs ship
  the `picasu` binary alone with both tools documented as prerequisites.
- 2026-09-27 — Iteration 4 done. Manifest vocabulary gained `iptc` and `text`
  plus a `FIELD_SOURCES` cross-product table (a field may only be claimed from
  a source it can live in — closes `container: [embedded]`, the open question
  `test-exif-xmp-handling` left). Claim flips per decision 6: jpeg
  `iptc: [embedded]` + pinned fixture `jpeg-48x32-iptc` (ImageMagick pixels +
  ExifTool-written IIM, no XMP packet — proof that tags/description arrive via
  IIM alone); png `xmp: [embedded, sidecar]` + `text: [embedded]`, unsupported
  list emptied, pinned fixture `png-48x32-xmp-text` (checked-in python stdlib
  generator: compressed `iTXt` XMP + `tEXt` Description/Comment/Source —
  re-verified compressed, no plaintext packet); tiff/webp/mp4/mov claims
  unchanged after measurement (their fixtures carry no IIM/XMP/text). Retired
  reader names in fixture `expectedMetadata` fixed (`DateTime`→`ModifyDate`,
  `DateTimeDigitized`→`CreateDate`) with a test forbidding retired names.
  **Manifest policy change (ratified):** a `generated` format may now pin
  additional fixtures backing claims its generator cannot produce — the old
  "generated must not reference pinned fixtures" exclusivity made the two new
  fixtures unregistrable; the teeth moved to an extension-match check, and
  `pinned ⇔ snapfab-cannot-encode` is untouched. IPTC2/3 fix: all three
  numbered IIM group names feed the native mapping (`IIM_GROUPS`, standard
  record first, matching ExifTool's low-priority marking), `NATIVE_KEYS` is a
  constant-derived cross product so the bucket complement moves with it; a
  real two-record JPEG test asserts ExifTool actually reports `IPTC2` before
  the mapping assertion. Stale `process/xmp.rs` comment fixed in the
  randomized scenario. Mutations: 27/27 killed. Open: IIM-in-PNG (a `zTXt`/
  `iTXt`-wrapped IIM record) unmeasured — no claim made; the flaky
  `a_killed_child_is_replaced...` race (`/proc` visibility of a freshly
  spawned child, seen once under heavier parallel ExifTool use) — fix in
  Iteration 5. Gates: `cargo test -p snapfab` 72, `-p picasu` 418 (+3
  integration, 1 ignored), `just backend-check/utils-check/docs-check`,
  `cargo deny` — all pass; no frontend changes.
- 2026-09-27 — Iteration 3 done. The read-only bucket ships as
  `furtherMetadata` (Rust `further_metadata`, sidebar section "Further
  metadata"): an image-persisted `BTreeMap` keyed `Group:Tag` whose split is
  documented on `process::xmp::map_further_fields` — in: `XMP-*` (all
  namespaces), `IPTC`/`IPTC2`/`IPTC3`, `PNG` text chunks, minus the keys the
  native mapping consumed (a `NATIVE_KEYS` complement, so nothing appears
  twice); out: the EXIF family (already `exifVec`), `File:`/`System:`/
  `ExifTool:` read-time trivia, `Composite:` (measured duplicates of shown
  values), `JFIF:`, maker notes, ICC matrices, the PNG container's own
  properties; `Photoshop:` excluded on judgement and recorded as the next
  candidate. Fixture: snapfab's JPEG IPTC writer gained a gated
  `further_iptc: true` (IIM 2:80 By-line, 2:90 City, 2:116 CopyrightNotice) so
  one file exercises both halves of the split. Surface: detail route +
  utoipa description (the schema types `abstractData` as an opaque object, so
  the field cannot appear as a schema property — an OpenAPI-contract
  hardening item), lean-list strip extended, rebuild/reindex semantics pinned
  to match `exif_vec`, and `show_metadata: false` now clears the bucket too
  (share privacy — without it the detail route leaked unmodelled metadata).
  Frontend: `ItemFurtherMetadata.vue` read-only rows, typed schemas, store
  merge + vitest, Playwright `image-further-metadata-sidebar` (asserts zero
  edit affordances). **Migration consequence:** `METADATA_SCHEMA_VERSION` 2→3
  (bitcode field addition decodes as EOF) — existing libraries get the
  actionable `rebuild_required` message and need `POST /post/rebuild` after
  this build; `docs/database.md` updated. Carry-overs recorded: (a)
  `IPTC2`/`IPTC3` records bypass Iteration 2's native mapping — ExifTool files
  an IIM record outside its standard location under a numbered group name
  (non-standard _location_, not record version; wording corrected in
  Iteration 4) — so such captions/keywords land in the bucket instead of
  `description`/`tags`; fix in Iteration 4's mapping pass; (b) `value_text` duplicates
  `process::exif::json_value_to_string` (private) — hoist in Iteration 5;
  (c) share-mode (`show_metadata: false`) has only a unit test, no Playwright
  scenario. Gates: `cargo test -p picasu` 413 (+3 integration, 1 ignored),
  `-p snapfab` 63, `just backend-check/utils-check/frontend-check/docs-check`,
  `cargo deny`, full `just frontend-playwright` **40/40** (note: the recipe
  hardcodes the default `target/debug/picasu`; under the relocated
  `CARGO_TARGET_DIR` it needs `PICASU_BINARY=<cache>/debug/picasu`, same as
  Iteration 1's report).
- 2026-09-27 — Iteration 2 done. `process::xmp` is now two layers: a read
  layer (`native_metadata_for` / `read_xmp_packet`) owning the sidecar rule —
  a sidecar's _existence_ takes the XMP source whether or not it parses, its
  XMP replaces the image's packet while the image's IPTC/PNG-text keep filling
  gaps — and a pure mapping layer (`map_native_fields`) unit-tested on
  recorded ExifTool payloads. Measured contract: `description` ←
  `XMP-dc:Description` → `IPTC:Caption-Abstract` (IIM 2:120) →
  `PNG:Description`; `title` ← `XMP-dc:Title` → `IPTC:ObjectName` (IIM 2:05,
  not Headline 2:105 — ExifTool's own MWG reconciler pairs it that way) →
  `PNG:Title`; `rating` ← `XMP-xmp:Rating` only (arrives as JSON number or
  `4 stars`-style string, leading-integer parse, 0..=5 kept); `tags` = union
  of `XMP-dc:Subject` ∪ `IPTC:Keywords` (IIM 2:25); PNG text contributes no
  tags (no standard keyword chunk — measured and pinned). One ExifTool read
  per image now serves both `exifVec` and the native fields (the record is the
  currency; `GroupedMetadata`'s array flattening would lose comma-containing
  keywords); a sidecar is the only second read. `XmpData` renamed
  `NativeMetadata`. The byte-scan parser is gone: 20 dead tests inventoried —
  replacements are recorded-payload/file-based tests, scenario pins that stay
  green, or documented obsolescence (no-panic sweep, first-occurrence rule,
  empty-Alt leak cannot occur through ExifTool). Two scenarios flipped under
  decision 6 and were rewritten with the withdrawn contract stated in their
  headers; the three overturned pins of `test-exif-xmp-handling` are recorded
  there. Ratified from the report: the worker's additive `exif.rs` change
  (raw record as shared currency, `generate_exif_for_image` →
  `exif_map_from_record`) — justified and flagged proactively; `exifVec`
  byte-identical across all 395 pre-existing assertions. **Decision:** a
  non-UTF-8 keyword inside a packet reads as ExifTool's lossy decode (the
  `???`-style tag is indexed) — no app-side filtering heuristic; the engine's
  output is authoritative, and the old drop-the-whole-field behavior died
  with the scanner. Open: the scenario harness ignores body assertions in a
  non-last `call:`'s `then:` (found because it made both rewritten scenarios
  vacuous) — filed in `.plan/scenario-harness-debt.md`; video files now also
  get a native-fields ExifTool read (sidecar XMP for video was already
  claimed; embedded uuid XMP remains unclaimed until Iteration 4 measures
  it). Gates: `cargo test -p picasu` 404 (401+3, 1 ignored), `-p snapfab` 62,
  `just backend-check/utils-check/docs-check`, `cargo deny`, targeted
  Playwright (4 format flows + 5 sidebar flows) — all pass.
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
