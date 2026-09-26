---
status: open
type: feature
priority: medium
area: testing
---

# Common-format metadata coverage

## Goal

Make metadata extraction and indexing behavior explicit and tested for every
file format Picasu claims to support. Tests are not sufficient by themselves:
several requested formats expose implementation gaps that must be fixed or
explicitly rejected before extraction assertions can be meaningful.

## Current support boundary

The backend currently accepts these filename extensions:

- Images: JPEG (`jpg`, `jpeg`, `jfif`, `jpe`), PNG, TIFF (`tif`, `tiff`),
  WebP, and BMP.
- Videos: GIF, MP4, WebM, Matroska (`mkv`), MOV, AVI, FLV, WMV, and MPEG.

HEIF/HEIC and AVIF are not currently supported and are explicitly out of scope
for this plan. They should remain rejected by the extension allowlist; do not
add positive metadata fixtures or implementation work for them now.
MP4 and MOV tests require both `ffmpeg` and `ffprobe`.

## Test layers

1. **Unit: parser placement and error behavior**
   - Test XMP extraction from real or representative container bytes for:
     - JPEG APP1 XMP
     - TIFF XMP placement
     - MP4 UUID-box XMP
     - MOV QuickTime metadata placement
   - Do not require embedded PNG XMP extraction; PNG embedded XMP remains
     unsupported. Test sidecar XMP separately where that contract is retained.
   - Test sidecar XMP independently from container bytes.
   - Test malformed XML, truncated packets, missing fields, and packets after
     unrelated container data.
2. **Integration: metadata → index → API**
   - Add one real fixture scenario per supported image format covering
     dimensions, EXIF where supported, XMP fields, thumbnail generation, and
     persisted metadata.
   - Add MP4/MOV scenarios covering `ffprobe` format/stream metadata, XMP,
     dimensions, and thumbnail generation.
   - Assert the observable API response and metadata detail, not internal
     parser state.
3. **Negative integration: upload and index boundaries**
   - Empty file with a supported extension.
   - Truncated image and truncated video.
   - Random bytes with a supported extension.
   - Valid image bytes under a different supported image extension.
   - Valid MP4/MOV bytes under a different supported video extension.
   - Corrupt EXIF or XMP inside an otherwise decodable image.
   - Reject misnamed files by default, and accept them when
     `validate_upload_content` is disabled. The error must include the claimed
     filename/extension and the detected content type.
   - Reject unidentifiable content regardless of `validate_upload_content`, so
     the setting has no effect on bytes that cannot be identified at all.
4. **UI: representative file types**
   - Run a small cross-format smoke matrix through upload, gallery rendering,
     metadata sidebar, and deletion.
   - Do not duplicate every API assertion in Playwright; reserve UI tests for
     regressions involving routing, controls, and visible state.

## Format matrix

| Format    | Current status         | Metadata contract                     | Fixture strategy                                                          |
| --------- | ---------------------- | ------------------------------------- | ------------------------------------------------------------------------- |
| JPEG      | Supported              | EXIF + APP1 XMP + IPTC IIM            | Keep snapfab; retain real fixture scenario                                |
| PNG       | Supported              | EXIF; embedded XMP unsupported        | Keep snapfab; test EXIF, dimensions, and sidecar behavior only            |
| TIFF      | Accepted; no generator | EXIF, dimensions, XMP placement       | Small pinned TIFF fixture with provenance                                 |
| WebP      | Accepted; no generator | Establish EXIF/XMP support contract   | Small pinned WebP fixtures; do not claim fields before verified           |
| MP4       | Accepted; no generator | ffprobe metadata + UUID-box XMP       | Tiny deterministic ffmpeg file plus pinned metadata fixture if needed     |
| MOV       | Accepted; no generator | ffprobe metadata + QuickTime metadata | Tiny deterministic ffmpeg MOV file plus pinned metadata fixture if needed |
| HEIF/HEIC | Rejected; out of scope | No metadata contract                  | Do not add extraction fixtures or tests                                   |
| AVIF      | Rejected; out of scope | No metadata contract                  | Do not add extraction fixtures or tests                                   |

## Fixture policy

- Keep snapfab as the source for deterministic JPEG and PNG images.
- Extend snapfab only for deterministic formats where its existing encoder and
  metadata writer can produce faithful output without a new external toolchain.
- Use small, pinned, checked-in fixtures for TIFF, WebP, MP4, and MOV. Store a
  manifest containing source URL/repository, commit or version, SHA-256, license,
  expected metadata, and intended failure behavior.
- Prefer upstream test data with clear redistribution terms, such as libavif
  and libheif test assets. For ordinary MP4/MOV, generate tiny files with
  ffmpeg; use pinned files when testing a metadata writer or unusual box.
- Do not use arbitrary production images. Keep fixtures minimal and document
  whether they are generated, synthetic, or third-party.

## Implementation decisions

- HEIF/HEIC and AVIF remain unsupported and rejected by extension policy.
- PNG embedded XMP remains unsupported. Do not add extraction work or positive
  embedded-XMP assertions for it in this scope.
- Misnamed files are rejected by default and tolerated when
  `validate_upload_content` is disabled. The rejection notification must include
  the claimed filename/extension and the detected content type.
- Content that cannot be identified at all is rejected regardless of
  `validate_upload_content`.
- MP4/MOV UUID metadata and ffprobe metadata are separate contracts. Tests
  must distinguish them and report which source supplied each field.
- Corrupt metadata behavior must be explicit: test current fallback behavior
  first, then change it only with a separate product decision.

## Iterative implementation plan

Each iteration follows the same contract: a worker sub-agent first writes a
minimal regression test and demonstrates the expected failure; the agent then
implements the smallest production change; the parent session reviews the diff
and runs focused tests before the full applicable checks. Agents must not edit
unrelated files, change established semantics silently, or commit independently.

### Iteration 0 — Define the capability manifest

- **Owner:** test-infrastructure worker.
- **Tests first:** validate a manifest schema containing format, extension,
  content signature, supported metadata fields, sidecar behavior, and expected
  failure class. Add a selftest that rejects an incomplete manifest entry.
- **Implementation:** add a repository-owned manifest used by scenario
  fixtures and randomized selection. Keep it independent of UI code.
- **Gate:** manifest unit tests, `just plan-lint`, and the existing scenario
  loader tests.

### Iteration 1 — Enforce misnamed-file rejection

- **Owner:** backend API worker.
- **Tests first:** add upload scenarios for JPEG bytes named `.png`, PNG bytes
  named `.jpg`, MP4 bytes named `.mov`, and a supported extension with random
  bytes. Assert status, error code, claimed filename, claimed extension, and
  detected type in the user-facing message.
- **Implementation:** centralize signature detection and reuse it for
  filesystem indexing, where non-matching content is an unrecognized file to
  ignore and log rather than a scan failure.
- **Upload decision order:** the declared extension yields the expected content
  identifier from the supported-format table, and an unknown extension is
  rejected. The content is then identified, which yields either a real type or
  `unrecognized`. Unidentifiable content is always rejected. A _mismatch_ — the
  content is identified but is not the type the extension claims — is the one
  case `validate_upload_content` governs, so a mislabeled file can be tolerated
  by disabling it.
- **Carry-over:** the hardcoded `jpeg | png` list in
  `backend/src/tests/backend_api.rs` is generator-side — it builds a
  `snapfab::PhotoSpec` from a scenario — so replace it with a manifest lookup.
- **Gate:** focused API scenarios, backend unit tests, and the frontend toast
  assertion for the notification text. Do not add a new error type solely for
  the test; the contract must describe the existing error path.
- **Deferred from this iteration:**
  - The MP4-bytes-named-`.mov` scenario. No MP4 fixture can be produced yet:
    snapfab generates only JPEG/PNG and `raw_file` writes UTF-8 text, so a
    binary fixture mechanism is needed. Belongs with Iteration 4.
  - A scenario proving the flag still _permits_ an upload. It would need
    unrecognized-but-decodable bytes, which no fixture can express today.
  - The frontend toast assertion named in the gate above.

### Iteration 2 — Solidify JPEG and PNG contracts

- **Owner:** backend metadata worker.
- **Tests first:** retain the JPEG APP1/IPTC scenario; add PNG EXIF, dimensions,
  thumbnail, and sidecar-XMP scenarios. Add a test proving embedded PNG XMP is
  not promised and does not silently become a required field.
- **Implementation:** only change extraction if a failing test demonstrates a
  real JPEG/PNG regression. Do not add PNG embedded-XMP decompression.
- **Carry-over:** record the limitation of the APP1-unaware byte scan in
  `backend/src/process/xmp.rs`, so the JPEG embedded-XMP claim is not read as a
  guarantee that compact XMP is detected.
- **Gate:** snapfab tests, metadata API scenarios, and the targeted frontend
  metadata sidebar scenario.

### Iteration 3 — Add TIFF and WebP real fixtures

- **Owner:** fixture/format worker.
- **Tests first:** add pinned, provenance-documented TIFF and WebP fixtures
  and one indexing scenario for each. Assert only fields demonstrated by the
  fixture and current decoder: dimensions, thumbnail, EXIF where verified, and
  sidecar XMP.
- **Implementation:** extend the `image` decoder features only if the fixture
  fails for a supported format; do not infer metadata support from extension
  acceptance.
- **Gate:** fixture manifest validation, backend API scenarios, and `just
backend-check`.

### Iteration 4 — Add MP4 and MOV integration coverage

- **Owner:** video backend worker.
- **Tests first:** add tiny deterministic ffmpeg-generated MP4 and MOV fixtures,
  then assert ffprobe format/stream metadata, dimensions, thumbnail, and any
  verified XMP source. Add a missing-tool test that reports a clear diagnostic.
- **Implementation:** keep ffprobe and raw XMP responsibilities separate. Do
  not claim UUID-box support until a real fixture demonstrates it.
- **Gate:** video scenarios with `ffmpeg`/`ffprobe` present, backend tests, and
  release-build compilation.

### Iteration 5 — Define corrupt/missing metadata fallback

- **Owner:** backend metadata worker.
- **Tests first:** cover empty files, truncated media, corrupt EXIF/XMP inside a
  decodable image, corrupt sidecars, and missing optional fields. Assert stable
  API responses and no panics.
- **Implementation:** preserve the current fallback where it is safe. Any change
  from raw to sidecar to default precedence requires a separate decision and a
  failing test that demonstrates the old behavior is wrong.
- **Gate:** negative API scenarios, metadata unit tests, and a reindex test that
  proves the DB cache can be reconstructed from raw plus sidecar data.

### Iteration 6 — Implement seeded randomized scenarios

- **Owner:** test-infrastructure worker.
- **Tests first:** add harness tests for deterministic seed replay, capability
  filtering, logged format selection, and exclusion of unsupported formats.
- **Implementation:** add fixed CI seeds first; add broader nightly seeds only
  after the deterministic matrix is green. Randomize input selection, never
  expected outcomes.
- **Gate:** frontend and API scenario suites with a recorded seed manifest.

### Iteration 7 — UI cross-format smoke coverage

- **Owner:** frontend worker.
- **Tests first:** add one small upload → gallery → metadata → delete flow for
  each capability group: still image, TIFF/WebP image, and MP4/MOV video.
- **Implementation:** keep UI scenarios behavior-focused; do not duplicate
  backend parser assertions in Playwright.
- **Gate:** targeted Playwright tests, then the full frontend suite.

## Sub-agent coordination rules

- Assign one bounded subsystem per worker and require exact file ownership in
  the task prompt.
- Workers report the red test, implementation files, focused verification, and
  unresolved contract questions.
- The parent session reviews each diff before starting the next iteration.
- A worker may not mark a format supported merely because a fixture was added.
- If a test requires a product decision, stop that iteration and record the
  decision in this plan before implementation continues.

## Randomized scenario rollout

Randomization should complement, not replace, the deterministic matrix.

1. Add a seeded format selector to the API/UI scenario harness.
2. Choose from a manifest of formats that have verified fixtures and expected
   capabilities; do not randomly select HEIF/AVIF or unsupported metadata
   claims.
3. Log the selected seed and format in the test result.
4. Keep a small fixed seed set in CI for reproducibility, plus a nightly
   broader seed set for combinations.
5. Randomize only the input file type/content, not the assertions. Each selected
   format must have a capability contract: supported fields, fallback fields,
   and expected rejection behavior.
6. Run the same randomized format across upload, indexing, metadata, gallery,
   and deletion scenarios where the behavior is format-independent.

## Acceptance criteria

- Every currently supported format has one real-fixture integration scenario.
- Misnamed, empty, truncated, random-byte, and corrupt-metadata cases have
  explicit expected outcomes.
- HEIF/HEIC and AVIF remain rejected by policy and are not included in positive
  format coverage.
- MP4/MOV tests clearly require ffmpeg/ffprobe and skip or fail with an explicit
  environment diagnostic when unavailable.
- Randomized scenarios use recorded seeds and only formats with declared
  capabilities.
- `just check`, backend tests, frontend tests, and the relevant Playwright
  scenarios pass.

## Progress

- 2026-09-25 — Indexer side of Iteration 1. `model::media::classify_media_file`
  combines the extension allowlist with content detection, and the album scan,
  watcher, and DB rebuild now skip and log anything it rejects rather than
  counting it as a failure. `is_valid_media_file` deliberately stays
  extension-only because the watcher needs it for `Remove` events, where the
  file is already gone. Both new scenarios were mutation-checked: removing the
  gate fails them. Exact index counters are deliberately not asserted, because
  indexing writes a thumbnail into the scanned directory and the count is not
  stable.
- 2026-09-25 — Iteration 1 upload path. Added `backend/src/process/format.rs`
  with the backend's own signature table, built on `infer` and verified against
  the crate's matcher order and canonical extensions. A file whose content
  contradicts its declared type is now rejected regardless of
  `validate_upload_content`; only content matching no signature stays behind
  that flag, and recognized-but-unsupported formats (HEIF/AVIF) are reported by
  name instead of degrading to "not recognized". Error messages now carry the
  claimed filename, claimed extension, and detected type. Replaced
  `upload_content_type_validation_opt_out.yaml`, which asserted the removed
  behavior, with scenarios covering the new one. Deferrals recorded above.
- 2026-09-25 — Second Iteration 0 review round: full validation-branch
  coverage, the `unsupportedMetadataFields` representation, the manifest scope
  note, and the `CapabilityError` traits. Corrected a factual error it surfaced:
  `xmp.rs` resolves sidecars and scans embedded packets with no format dispatch,
  so JPEG also supports sidecar XMP and the manifest now claims it. Mutation
  checks confirmed each validation rule is load-bearing; the duplicate-entry
  test was rewritten after the first check showed it was masked by the
  contradiction check.
- 2026-09-25 — Iteration 0 implemented on `feat/format-capability-manifest`.
  Added `utils/snapfab/capabilities.json` with schema and semantic validation,
  signature decoding, format/extension lookup, and manifest-driven randomized
  fixture selection. A completeness test fails if the manifest declares a format
  snapfab cannot generate, and `generate_photo` rejects explicitly requested
  non-generatable formats instead of silently emitting JPEG bytes. A backend
  test asserts every manifest extension is accepted by the upload/index
  allowlist. The manifest only claims metadata the backend reads (JPEG EXIF +
  XMP, PNG EXIF, PNG sidecar XMP); the unsupported JPEG IPTC claim was removed.
  The snapfab binary now consumes the library crate instead of recompiling the
  modules. Focused tests, `just utils-check`, and `just plan-lint` pass. No
  scenario-loader unit test exists yet; the loader is covered by the Playwright
  interpreter spec.
