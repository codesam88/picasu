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
   - Reject misnamed files regardless of `validate_upload_content`. The error
     must include the claimed filename/extension and the detected content type.
   - Keep the existing `validate_upload_content` setting covered separately for
     files whose extension is supported and whose content is unrecognized.
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
- Misnamed files are rejected regardless of `validate_upload_content`. The
  rejection notification must include the claimed filename/extension and the
  detected content type.
- MP4/MOV UUID metadata and ffprobe metadata are separate contracts. Tests
  must distinguish them and report which source supplied each field.
- Corrupt metadata behavior must be explicit: test current fallback behavior
  first, then change it only with a separate product decision.

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
