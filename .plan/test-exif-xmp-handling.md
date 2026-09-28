---
status: backlog
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
- **Identification:** `infer` alone, fingerprinting the leading bytes. ffprobe
  is not used to identify content. It was measured and rejected for the purpose:
  it reports a whole container group as one name (`mov,mp4,m4a,3gp,3g2,mj2` for
  mp4/mov/m4v, `matroska,webm` for mkv/webm, `asf` for wmv), so it would lose
  the distinction `infer` makes by brand and DocType. `infer` fingerprints every
  format in the table. The one video it cannot fingerprint is an MPEG transport
  stream, which is not an accepted extension.
- **Carry-over:** the hardcoded `jpeg | png` list in
  `backend/src/tests/backend_api.rs` is generator-side — it builds a
  `snapfab::PhotoSpec` from a scenario — so replace it with a manifest lookup.
- **Gate:** focused API scenarios and backend unit tests. Do not add a new error
  type solely for the test; the contract must describe the existing error path.
  The message reaches the UI through an unmodified pass-through and an
  upload-error toast is already asserted by
  `upload-options-auto-rename-off-rejects.yaml`, so no frontend test is added
  for this iteration. The flag's permissive path is covered by
  `upload_misnamed_content_allowed_when_validation_off`.
- **Deferred:** the MP4-bytes-named-`.mov` scenario. No MP4 fixture can be
  produced yet: snapfab generates only JPEG/PNG and `raw_file` writes UTF-8
  text, so a binary fixture mechanism is needed. Belongs with Iteration 4.

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

## Follow-up Issues

The format-coverage implementation is complete, but review identified the
following source-of-truth and edit-semantics issues. They are implemented one
issue at a time; status is recorded under Follow-up Status below.

### Rebuild Must Reconstruct Metadata

`POST /post/rebuild` currently recreates identity and duplicate tables but does
not run the metadata pipeline. After the metadata schema migration, the
operator is told to rebuild, yet `exifVec`, tags, descriptions, ratings, and
`furtherMetadata` remain empty until a separate reindex.

The intended design is one shared indexing pipeline with two orchestration
modes:

- Incremental index is additive/on-demand: preserve existing asset identity,
  discover new or changed files, process their metadata and derived data, and
  reconcile stale paths.
- Rebuild resets identity, duplicate, and metadata tables, walks the complete
  filesystem, recreates albums and media identity records, then runs the same
  metadata/index pipeline for every discovered media asset before reporting
  completion.

Rebuild should reuse the internal indexing workflow, not literally call the
HTTP incremental-index endpoint: incremental indexing assumes existing
identity state, while rebuild must create that state first. It must clear
`METADATA_TABLE` as well as the identity tables because rebuilt asset IDs are
currently regenerated. Rebuild failures must retain per-file diagnostics and
must not report a usable cache until metadata processing has completed.

Required coverage:

- Rebuild reconstructs EXIF, native fields, further metadata, and sidecar
  overrides from the file system through the shared indexing pipeline.
- A rebuild after `METADATA_SCHEMA_VERSION` changes leaves usable metadata,
  not only identity records.
- Rebuild failure states identify files that could not be reprocessed.

Relevant code: `backend/src/process/rebuild.rs`,
`backend/src/storage/ser_de.rs`, and
`backend/tests/scenarios/rebuild_reconstructs_identity_but_not_metadata.yaml`.

### Sidecar Edits Must Override Embedded Metadata

The current rule replaces only the XMP source. Tags are unioned from XMP and
embedded IPTC, so removing an embedded IPTC keyword through `/put/edit_tag`
can appear to work in the cache and then return after reindexing. The future
contract needs an explicit override/tombstone model, or a rule that a managed
sidecar field is authoritative, including an intentionally empty tag set.

Required coverage:

- Add an embedded IPTC keyword, remove it through the edit API, reindex, and
  verify that it stays removed.
- Verify add, remove, clear, description, and rating edits against both
  embedded XMP and IPTC sources.
- Preserve the distinction between user-managed fields and unmodelled
  read-only metadata.

Relevant code: `backend/src/process/xmp.rs`,
`backend/src/process/xmp_write.rs`, and the PUT edit handlers.

### Preserve Unmanaged Sidecar Metadata

`write_sidecar_for` replaces the entire `.xmp` file with a minimal packet for
tags, description, rating, and album title. Editing a managed field can delete
unmodelled XMP properties that are shown through `furtherMetadata`.

The future write path should perform a read-modify-write using ExifTool or an
equivalent established metadata writer, changing only managed properties and
preserving unknown namespaces, properties, and packet data where possible.

Required coverage:

- Seed an XMP sidecar with managed and unmanaged properties.
- Edit one managed property and verify the unmanaged properties remain.
- Verify atomic replacement and behavior when the existing sidecar is
  malformed.

Relevant code: `backend/src/process/xmp_write.rs`.

### Sidecar Write Failure Must Not Diverge the Cache

The edit handlers log sidecar write errors but still persist the mutated redb
payload and return success. A failed write therefore makes the cache claim an
edit that is not present in the source-of-truth sidecar; the next reindex
reverts it.

Choose and test one explicit contract:

- Return an error and leave the cached metadata unchanged, or
- Keep the operation successful only after the sidecar write succeeds and
  update the cache transactionally afterward.

Required coverage includes permission/read-only failures for tag,
description, and rating edits.

Relevant code: `backend/src/router/put/edit_tag.rs`,
`edit_description.rs`, `edit_rating.rs`, and
`backend/src/process/xmp_write.rs`.

### Missing ExifTool Runtime Behavior

The current indexer logs an ExifTool startup failure and stores empty metadata,
while malformed metadata also falls back to empty fields. These cases should
be distinguishable operationally. Decide whether missing ExifTool is a hard
indexing/deployment failure or a persistent health error rather than silently
accepting empty metadata.

Required coverage:

- Missing executable during import/index.
- ExifTool process failure after startup.
- Malformed metadata in an otherwise decodable file.
- Stable API status and actionable operator diagnostics for each case.

Relevant code: `backend/src/process/exif.rs` and
`backend/src/process/index.rs`.

### Additional Known Scope Gaps

- `furtherMetadata` is currently image-only; video metadata remains ffprobe
  data in `exifVec` with no equivalent further bucket. Decide whether and how
  video container metadata should be exposed.
- The scenario harness still has the silent non-last-`then` assertion issue,
  dead `serve_image_ok` behavior, failed-index assertion limitations, and
  random-port race documented in `.plan/scenario-harness-debt.md`. These must
  be fixed or explicitly accepted before using new scenarios as release
  evidence.

## Follow-up Status

2026-09-28 — **Harness assertion integrity fixed (Gap 6a; five of the eight
`.plan/scenario-harness-debt.md` items).** Every `call:`'s inline `then:` now
runs status _and_ body _and_ file/serve assertions (non-list `then` is a hard
error), enabling 15 previously-dead assertions across 13 scenarios: 13 were
correct-and-passing, 2 were flawed YAML (missing sync point after a `write_file`;
`locateTo` is a snapshot position, not a constant) — none hid a product
failure. `serve_image_ok` implemented (compressed route, 200 + content type +
magic bytes, harness-minted hash token); the scenario passed once wired —
serving was never broken — and a product mutation makes it die. `wait_index`
gained `failed`/`{expect: …}` so failure states are assertable, pinned
end-to-end by `album_index_failed_when_every_matched_file_fails.yaml` (which
needed `truncate_file` as a `when` verb). `file_exists`/`file_absent`/`file.*`
now reject absolute interpolated paths with a diagnostic (two affected
scenarios corrected; the `upload_unindexable_removed` assertion was dead).
`asset_id_as` without `id_as` fixed and pinned (first draft masked by a
sibling `id_as` — caught and reworked). Selftests + mutations for each;
`docs/scenario-dsl.md` documents the now-enforced rules. Debt file annotated:
5 fixed, 3 open (schema.json enforcement, Playwright port race → Gap 6b,
snapfab CLI selector bug). Gates: `cargo test -p picasu` 493 (2 ignored),
snapfab 75, `just backend-check`, `cargo deny`, `just docs-check`.
2026-09-28 — **Toolchain vs malformed metadata separated (decision: hard
failure for toolchain).** All 12 `exiftool`-crate error variants are classified
by `process::exif::is_toolchain_variant`: HARD (propagates — index fails,
upload 500s, rebuild counts `metadataFailed` with the remedy, album scan
reaches its `failed` state, watcher logs) = binary missing, pipe `Io`
(measured: on the read path `Io` can only be stdin/stdout — ExifTool opens
files in its own process), stderr disconnect, dead child, `MutexPoison`
(also lets a poisoned _write_ session restart — accepted); SOFT (file keeps
indexing with empty metadata, unchanged) = process-rejection, `FileNotFound`,
JSON/UTF-8. Key measurement: ExifTool 13.59 reports most damaged files _inside_
the JSON with exit 0, so the SOFT classification arm is mostly unreachable on
this engine version and is pinned by injected variants + real corrupt
fixtures. Upload diagnosis fixed in the same gap: `classify_index_failure`
(a pure fn over the real error chain, which does reach the handler) maps
toolchain failures to **500 + the shared install remedy** (previously 400
"could not be decoded" — a misdiagnosis), while the decode branch keeps its
byte-identical 400 and file removal (invariant: a failed upload leaves no
unindexed file — recorded choice); the previously unpinned decode branch got
`upload_undecodable_removed.yaml`. Residuals: `POST /post/index/image` stays
fire-and-forget (log + status counters only); the remedy is returned to the
client deliberately (self-hosted, no paths/hashes leaked); `AlreadyIndexed`
outcome is unit-pinned only (needs a concurrent scan); a **new harness defect**
found — `file_absent`/`file_exists` accept `${data_path}`-prefixed paths that
resolve under image_home and pass vacuously (makes
`upload_unindexable_removed.yaml`'s assertion dead) → folded into Gap 6.
Mutations: classification swap (10 kills), remedy dropped (several), removal
predicate moved (scenario), upload branch swap (4+1). Gates: `cargo test -p
picasu` 487 (2 ignored), snapfab 75, `just backend-check`, `cargo deny`,
`just docs-check`.
2026-09-28 — **Sidecar override semantics implemented (amends decision 6 of
`.plan/exiftool-metadata-engine.md`).** `XmpSource` now carries provenance —
`Sidecar(Option<record>)` vs `Image(Option<record>)` — and a sidecar's
_existence_ (not its readability, not its content) selects the regime: with a
sidecar, tags/description/rating/title come from the sidecar alone (absent ⇒
empty, present-blank authoritative, no IPTC/PNG fall-through, no tag union);
without one, the import precedence (XMP > IIM > PNG-text, tags unioned)
stands. Two end-to-end reds proved the review bugs: an embedded IPTC keyword
removed via `/put/edit_tag` no longer resurrects on reindex, and a cleared
description stays cleared despite the file's IPTC caption. Nine pins flipped
(old → new contract stated in each comment, citing this section): corrupt
sidecar now withholds _every_ managed field (reversing decision 6's "IPTC
still fills"), unreadable sidecar same, blank-sidecar-value authoritative
while blank-embedded still falls through (the split needed provenance), plus
three rebuild scenarios that had asserted the union. Accepted trade-offs
pinned by name: an external partial sidecar suppresses the file's tags
(`an_external_partial_sidecar_suppresses_the_files_own_tags`, with the bucket
confirmed non-leaking via `NATIVE_KEYS` by-name exclusion); a corrupt sidecar
shows no managed fields even when the bytes carry them (repaired by the next
app edit). Residuals: video + sidecar inherits the rule but has no dedicated
test; scenario _filenames_ keep their old stems because `.plan` progress notes
reference them (their `name:` fields and headers state the current contract).
Mutations: restore-IPTC-union (11 kills), restore-scalar-fall-through (5),
consult-image-on-unreadable (1), no-key record ⇒ Image (2). Gates: `cargo test
-p picasu` 471 (2 ignored), snapfab 75, `just backend-check`, `cargo deny`,
`just docs-check`, `just frontend-test` (vitest 75, Playwright 40).
2026-09-28 — **Sidecar/cache divergence fixed.** All four edit endpoints
(tag/description/rating/album) now go through one request-level transaction,
`process::sidecar_edit::commit_metadata_edits`: capture each sidecar's bytes →
write all sidecars → on any write _or read_ failure roll back the already
written ones (bytes restored, created files removed) and return `ErrorKind::IO`
(500) without storing anything → store payloads only after every sidecar
landed. The failing item itself is not restored (temp+rename left it
untouched); a store failure after successful writes deliberately leaves
sidecar-ahead-of-cache, which reindex converges — the other direction was the
silent lie. Review round found and fixed a real bug: a capture (backup-read)
failure propagated without rollback, letting earlier writes survive a failed
request — now pinned by unit + scenario tests (`...rolls_back_the_batch`)
and mutation-checked (rollback removed → both die). Harness gained a
generalized `chmod: {path, octal}` when-verb (file or dir, mode restored even
on scenario panic via `remember_path_mode`/`restore_path_modes`) and a `not_contains` JSON assertion
needed to say "cache unchanged" about a removal. Residuals: the frontend does
not roll back its optimistic tag update on a 5xx (display-only, converges on
refresh — follow-up); OpenAPI `responses(...)` still does not enumerate the
reachable 500 (`.plan/openapi-contract-hardening.md`); concurrent edits of one
asset can interleave (documented in the module header); chmod-based tests need
a non-root runner (CI and this machine are). Gates: `cargo test -p picasu`
465 (+6 unit, +6 scenarios, 2 ignored), snapfab 75, `just backend-check`,
`cargo deny`, `just docs-check`.
2026-09-28 — **Unmanaged-sidecar preservation implemented.** `write_sidecar_for`
is now a read-modify-write through ExifTool (`exif::write_xmp_properties`,
sharing the reader's thread-local `-stay_open` session): only the managed
properties (`dc:subject`, `dc:description`, `xmp:Rating`, album `dc:title`) are
named, so every other property/namespace survives. Measured semantics: bag
assignments replace per element (no stale-tag accumulation; cleared tags delete
the property), scalars clear with `-TAG=`, multi-line values are staged through
a sibling file, `-overwrite_original` is mandatory. Photo title stays
unmanaged (never named); album title is managed including removal-on-clear.
Malformed sidecars: ExifTool refuses non-XMP byte-identically → controlled
managed-only fallback overwrite with an error log; transport/dependency
failures propagate without touching the file (the cache-divergence gap's
territory). Residuals pinned/documented: valid-XML-that-is-not-XMP is replaced
silently with no signal (`a_sidecar_of_xml_that_is_not_xmp_is_replaced_without_being_detected`);
sidecars are property-stable but not byte-stable across edits; sidecar writes
now require `exiftool` (consistent with the engine swap); an intermittent
~3% write-then-read failure in tests (3/~90 runs, one captured as an empty
read) could not be root-caused or reproduced in 150+ later runs — tests now
surface read errors instead of defaulting, worth watching. Mutations: drop-RMW
(10 kills), `+=` append (5), no-malformed-fallback (4), album-title skip (4),
photo-title-managed (1). Gates: `cargo test -p picasu` 452 (+3 scenarios,
+27 tests, 2 ignored), snapfab 75, `just backend-check`, `cargo deny`,
`just docs-check`.
2026-09-28 — **Rebuild gap implemented.** One shared metadata pipeline
(`process::index::process_media_info`) with two orchestration modes: the
incremental index resolves identity through open/hash/deduplicate, the
filesystem rebuild now clears `METADATA_TABLE` alongside the identity tables,
recreates identity from the walk, and runs the same pipeline per media file
(`workflow::index_media_file`). Per-file failures are recoverable and reported
(`metadataIndexed`/`metadataFailed`/`metadataFailures`, details capped at 100,
uncapped count); a rebuild that only reissues identity reads as
`metadataIndexed: 0` rather than success-with-empty-metadata. The stale
`rebuild_reconstructs_identity_but_not_metadata.yaml` pin was replaced by three
scenarios (reconstruction, idempotence, stale-payload drop) plus 4 unit tests;
mutation-checked (pipeline removed → 13 tests fail; table clear removed → the
row-identity unit test fails). Residuals: rebuild is single-threaded (one
ExifTool read + thumbnail per file — slow for very large libraries, concurrency
deferred), album payloads stay default-only (unchanged), rebuilt videos stay
`pending: true` exactly like indexed ones (transcode is a separate task).
2026-09-28 — Core format coverage and ExifTool integration are complete. The
plan is moved to `backlog` to record the source-of-truth/edit-semantics work
above.

## Progress

- 2026-09-27 — Three pins overturned by `.plan/exiftool-metadata-engine.md`
  (ExifTool engine swap; decisions recorded there):
  1. **PNG embedded XMP is now supported.** `pinned_png_compressed_embedded_xmp_is_not_extracted`
     (byte-scan, no decompression) was replaced in that plan's Iteration 2 by
     `a_png_with_a_compressed_itxt_packet_yields_its_tags` — a genuinely
     compressed `iTXt` fixture extracted through the engine. The byte-scan
     parser those tests guarded no longer exists.
  2. **Sidecar precedence narrowed to the XMP source.**
     `corrupt_sidecar_suppresses_a_readable_embedded_packet` and the scenario
     `corrupt_xmp_sidecar_suppresses_embedded_xmp` asserted that a bad sidecar
     suppresses _everything the file carries_; the new contract is "a sidecar
     replaces the XMP source only — IPTC and PNG-text still fill native
     fields". The scenario was rewritten (its embedded-keyword assertion is now
     `present`, arriving via IIM 2:25) and the control
     `sidecar_xmp_is_authoritative_over_embedded_xmp` gained a description
     assertion to keep demonstrating authority.
  3. **JPEG IPTC is read** (was "the backend has no IPTC reader"): IPTC IIM
     2:25/2:120/2:05 now feed tags/description/title under the XMP > IPTC >
     text precedence; the manifest claim flip lives in the new plan's
     Iteration 4.
     The Iteration 5 "empty `rdf:Alt` leaks raw markup" defect and the byte-scan
     truncation/UTF-8 pins died with the parser and are documented as obsolete in
     that plan's Iteration 2 report.
- 2026-09-27 — Plan complete (Iterations 0–7), `status: done`. Final gates
  run on this commit: `just check` exit 0, `just test` exit 0 (backend 379 +
  integration, snapfab 62, vitest 74, Playwright 39), `just plan-lint` clean.
  Acceptance criteria: matrix formats (JPEG, PNG, TIFF, WebP, MP4, MOV) each
  have a real-fixture scenario; misnamed/empty/truncated/random-byte/corrupt
  cases have explicit pinned outcomes; HEIF/AVIF remain rejected with a
  manifest test forbidding claims for them; MP4/MOV hard-fail with an
  actionable toolchain diagnostic on both harnesses; randomized scenarios use
  the recorded `ci` seed set against manifest-declared capabilities only.
  Honest residual against criterion 1 as literally worded: the accepted video
  extensions _outside_ the format matrix (gif, webm, mkv, avi, flv, wmv, mpeg)
  have unit-level detection/allowlist coverage in `process/format.rs` but no
  real-fixture metadata scenario — they were never in this plan's matrix, and
  adding them would reuse the pinned-fixture and ffprobe machinery built here.
  Two environment incidents during the plan are tracked in
  `.plan/tmpfs-quota-test-runs.md` and `.plan/scenario-harness-debt.md` (the
  latter also holds the Playwright random-port race seen once in the final
  gate and confirmed flaky by an immediate green re-run).
- 2026-09-27 — Iteration 7 done. Three UI smoke flows added —
  `format-jpeg-upload-gallery-metadata-delete`, `format-tiff-…`,
  `format-mp4-…` — each: upload via file chooser → gallery tile count →
  sidepane (stored path + decoded dimensions) → soft delete → permanent delete
  → empty-state confirmation. Assertions are UI-visible state only (dialog/toast
  text, tile counts, row-type via the Rotate Left menu entry, sidebar path and
  `ItemSize`); no parser output, no HEIF/AVIF. The frontend `given` step could
  not place TIFF/WebP/MP4/MOV at all (`snapfab batch` encodes jpeg|png only),
  so `source_file` gained an optional `fixture: <manifest-id>` (mutually
  exclusive with `format`) backed by `pinnedFixtures.ts`, which verifies the
  manifest SHA-256 before copying, requires a declared extension, and hard-fails
  with an install diagnostic when a `container: [probe]` fixture is placed
  without ffmpeg/ffprobe. One flake source found and removed
  (`#col-ref > video` is bimodal against the transcode race; replaced with the
  menu-gated row-type probe), and one vacuity defect found by mutation: a
  `ui.count equals 0` after `navigate:` passes before the grid renders — the
  empty-state card text is now the wait. The same vacuity exists in the
  pre-existing `delete-photo-permanently.yaml`, and that scenario also carries
  a permanent coverage warning from a `PUT /edit_flags` path typo (should be
  `/put/edit_flags`); both left untouched as separate changes. Open: the
  upload path rewrites a `.jpg` extension to `.jpeg` (unverified whether
  intended — no API-layer pin found), and the video-delete thumbnail orphan
  (Iteration 6 question (a)) now also shows as a backend log line
  `Failed to delete thumbnail …/compressed/….jpg` during the MP4 flow. Gates:
  `just frontend-playwright` 39 passed (was 36), `just frontend-check`,
  `just docs-check`, `just plan-lint` — all pass; backend untouched
  (`git status backend/ utils/` empty, `capabilities.json` byte-identical).
- 2026-09-27 — Iteration 6 done. Correction first: the Iteration 0 note above
  claimed "manifest-driven randomized fixture selection" existed — it did not;
  only format/extension lookup existed. The selector is now real:
  `utils/snapfab/src/selection.rs` (`randomizable_formats` + `select(seed,
manifest)`), deterministic by an eligible list sorted by format name plus a
  hand-rolled splitmix64 mix — `SmallRng` was rejected because it is not
  stable across crate versions and a recorded seed must keep resolving to the
  same format when a dependency moves. Eligibility = manifest entry ∧ verified
  fixture ∧ `expectedFailureClasses ⊆ {none}`; HEIF/AVIF exclusion is
  structural (no encoder, no fixture → never eligible), backed by a second
  backend-side check that every selectable format is in the upload/index
  allowlist. Scenario opt-in is a top-level `randomize: {seeds: <set>}` block;
  a `random_media: <stem>` given verb materializes the selection and binds
  `${format}`/`${ext}`/`${mime}`; the selection is logged as a banner that
  survives panics (`run 3/6: seed=2 format=tiff ... source=pinned
fixture=tiff-48x32-exif`). Seed manifest `backend/tests/seeds.json`: `ci`
  = 6 fixed seeds covering each of the 6 formats exactly once with a golden
  `resolvesTo` table (a new format fails a test until a seed is added),
  `nightly` = 18 seeds (superset, 3× per format), default `ci`,
  `PICASU_RANDOM_SEEDS` overrides by set name or explicit list; no CI workflow
  change needed. Three randomized scenarios pin format-independent flows
  (index+metadata, upload+membership, delete+sidecar) across all 6 CI seeds —
  assertions deliberately exclude format-specific values. The UI harness was
  left unchanged after measurement: `executeGiven.ts` drives `snapfab batch`
  whose format enum is jpeg|png only, so a TS mirror would randomize over 2 of
  6 formats; cross-format UI coverage belongs to Iteration 7. Mutation checks:
  26/26 killed. Open questions: (a) deleting a video orphans its `.jpg`
  thumbnail — `AbstractData::Video::compressed_path()` returns the compressed
  `.mp4`, not the thumbnail the video pipeline wrote, so `delete_data` never
  removes it (image-only `thumb_absent` stays pinned; product decision:
  should deleting a video drop its thumbnail?); (b) a synthetic manifest whose
  heif entry carried a resolvable fixture would be selectable by snapfab alone
  — the backend allowlist check is the layer that rejects it, noted as the
  residual gap. Environment note: the shared `/tmp` 32 G tmpfs filled twice
  during this plan's work (Iteration 4 and here), and a `git stash pop` under
  quota pressure restored untracked files as 0 bytes (recovered from the
  stash; digests verified). Workers should set
  `TMPDIR=/home/codesam/.cache/picasu-scratch-tmp` when `/tmp` runs low. Gates:
  `cargo test -p picasu` (379), `cargo test -p snapfab` (62),
  `PICASU_RANDOM_SEEDS=nightly` randomized runs, `just backend-check`,
  `just utils-check`, `just docs-check`, `just frontend-check`,
  `just frontend-playwright` (36 passed), `just plan-lint` — all pass.
- 2026-09-26 — Iteration 5 done. Negative-coverage inventory: empty file,
  truncated image, truncated video, corrupt EXIF in a decodable image, corrupt
  sidecar, and missing optional fields were all uncovered and now have
  scenarios; random-bytes, misnamed, and unidentifiable-content items were
  already covered in Iterations 1/4 and were not duplicated. Binary damage is
  produced by two new harness transforms (`truncate_file`, `patch_file` in
  `backend_api.rs`) applied after fixtures are placed and before the scan —
  chosen over pinned corrupt blobs because a generated format cannot reference
  pinned fixtures under the manifest rules, and derived bytes keep the digest
  chain single-sourced. Unit layer: 9 new `xmp.rs` tests (malformed XML,
  truncated packets with an every-prefix no-panic sweep, sidecar precedence
  measured in all four combinations) and 2 `exif.rs` tests; production code
  untouched — the whole diff in those files is inside `#[cfg(test)]`. Reindex
  gate satisfied: `reindex_reconstructs_metadata_from_raw_and_sidecar.yaml`
  proves tags re-derive from a rewritten sidecar while geometry/EXIF re-derive
  from raw bytes. Mutation checks: 42 scenario mutations (40 killed, 2
  documented boundary survivors) + 12 production mutations (9 killed, 3
  documented). `backend/tests/schema.json` vocabulary/pattern corrected and
  its description now states it is unenforced. Findings recorded as open
  decisions: (1) a corrupt or unreadable sidecar silently suppresses a good
  embedded packet — pinned at unit and API level, fallback would be a
  precedence change needing this plan's decision; (2) an empty `rdf:Alt`
  description leaks the element's raw markup into the description field; (3)
  an index where every matched file fails reports `state: failed`, which the
  harness panics on, so that outcome is not scenario-assertable — decide
  whether undecodable-but-matching files should be skipped like unrecognized
  ones; (4) `POST /post/rebuild` reconstructs identity but never re-runs the
  metadata pipeline (`width: 0`, `exifVec: {}`, `tags: []` after rebuild),
  pinned by `rebuild_reconstructs_identity_but_not_metadata.yaml`; (5)
  `VideoMetadata::duration` is never populated (pinned by
  `video_duration_field_is_never_populated.yaml`); (6) `asset_id_as` on a
  `photo` given item is ignored unless `id_as` is also set — established
  two-key convention kept, one-line fix deferred; (7) `serve_image_ok` is a
  dead assertion — `image_serving_survives_album_move_v.yaml` currently asserts
  nothing on its final step; (8) `schema.json` remains unenforced. Gates:
  `cargo test -p picasu` (344), `cargo test -p snapfab` (50), `just
backend-check`, `just utils-check`, `just docs-check`, `just plan-lint`,
  prettier on `schema.json` — all pass.
- 2026-09-26 — Iteration 4 done. Pinned MP4/MOV fixtures added (~1.5 KB each,
  single H.264 frame, bitexact ffmpeg 8.0.1 command recorded verbatim in
  `fixtures[].source`, regeneration reproduces the recorded SHA-256). The
  manifest schema gained two constants — field `container` and source `probe` —
  so mp4/mov claim exactly `container: [probe]` + `xmp: [sidecar]` with
  `exif:embedded` and `xmp:embedded` unsupported; `exif` stays unclaimed
  because the backend never runs kamadak-exif on video (the API's `exifVec`
  for video is ffprobe output, stated in each fixture's `expectedMetadata`).
  Six scenarios added: per-format positive with per-field source attribution
  (ffprobe vs sidecar vs ffmpeg), two negative controls proving the container
  supplies no XMP (both fixtures verified to carry no Adobe uuid box), and the
  Iteration 1 deferral `upload_mp4_bytes_named_mov_*`. Missing-tool coverage
  is a hard precondition test (`video_metadata_requires_a_working_ffmpeg_and_ffprobe`)
  that fails with a diagnostic naming the missing binary — verified end-to-end
  under `env -i PATH=/nonexistent`. Mutation checks: 68 killed / 4 survived,
  the survivors documented as prose/redundant-by-construction.
  **Deferral resolution contradicts the plan's premise:** mp4 bytes named
  `.mov` are _accepted_, not rejected — `process/format.rs` shares extensions
  within a container family (isobmff) and `validate_upload_content` never
  enters the mismatch branch for family siblings. The scenario pins that
  contract (200, stored as `.mov`, `TAG:major_brand=isom` surviving in
  ffprobe output) plus a flag-off counterpart proving the flag is not what
  permits it. Deciding whether family sharing should instead be narrowed is a
  product decision left open. Other open items: no field×source cross-product
  validation in the manifest schema (`container: [embedded]` would parse);
  `VideoMetadata::duration` is never populated (`0.0`, with the real value in
  `exifVec.duration`) — belongs to Iteration 5; `backend/tests/schema.json`
  assertion-field pattern cannot express `exifVec.TAG:major_brand` and omits
  `fixture`/`thumb_exists`; UUID-box XMP remains unclaimed (needs a fixture
  that actually carries a uuid box); x264 regeneration is version-locked
  (digest test flags drift). Gates: `cargo test -p picasu` (321),
  `cargo test -p snapfab` (50), `just backend-check`, `just utils-check`,
  `just docs-check`, `just plan-lint`, `cargo build --release --features
embed-frontend --bin picasu` — all pass.
- 2026-09-26 — Iteration 3 done. Pinned fixtures added under
  `utils/snapfab/fixtures/` (TIFF 48×32 synthetic, python3-stdlib construction;
  WebP 48×32 lossless VP8L + VP8X/EXIF mux, ImageMagick/libwebp output, layout
  matching kamadak-exif's own `tests/exif.webp`), both ~5 KB with SHA-256,
  license, expected metadata, and failure class recorded in a new top-level
  `fixtures[]` array of `capabilities.json`. The manifest gained
  `fixtureSource` (`generated`/`pinned`) and `pinnedFixtures` per format;
  tiff/webp claim `exif: [embedded]` + `xmp: [sidecar]` only after end-to-end
  verification, with `xmp:embedded` unsupported. `every_manifest_format_is_generatable`
  became `every_generated_manifest_format_is_generatable`, paired with
  `repository_manifest_pins_exactly_the_formats_snapfab_cannot_encode`: snapfab
  _can_ encode TIFF/WebP pixels but neither encoder attaches EXIF, so pinned is
  the honest source until an EXIF-capable encoder exists. The scenario harness
  gained a binary-safe `fixture` given step (manifest-id → copy into IMAGE_HOME,
  digests verified in snapfab tests); `raw_file` stays UTF-8-only. Four
  scenarios added: per-format positive (ext, dimensions, EXIF dates,
  sidecar tags, thumbnail) and negative controls. Mutation checks: 24 scenario
  - 25 manifest mutations, all detected; two initially masked cases were
    restructured so each validation rule is pinned by its own error message.
    Open questions recorded: kamadak-exif reads a WebP `EXIF` chunk only as a
    bare TIFF block (a JPEG-style `Exif\0\0` prefix yields silently empty EXIF —
    product decision if files in the wild carry it); TIFF/WebP embedded XMP is
    unclaimed rather than disproven (the byte scan would likely match; belongs to
    the plan's parser unit layer); TIFF `contentSignature` is little-endian only;
    the fixture regenerator script is not checked in (provenance text covers
    re-derivation); digest verification runs in snapfab's test, not per-scenario
    placement. Gates: `cargo test -p picasu` (313), `cargo test -p snapfab` (45),
    `just backend-check`, `just utils-check`, `just docs-check`, `just plan-lint`
    — all pass.
- 2026-09-26 — Iteration 2 done. The PNG EXIF manifest claim was unbacked:
  `little_exif` writes PNG EXIF as an ImageMagick-style zlib `zTXt` "Raw profile
  type exif" chunk, which `kamadak-exif`'s PNG reader ignores (it only reads
  `eXIf`), so PNG fixtures carried no readable EXIF at all. snapfab's fixture
  writer now splices a spec-compliant `eXIf` chunk itself (CRC-verified, read
  back through `kamadak-exif`); a unit test rejects the raw-profile chunk
  returning. Added scenarios `png_metadata_exif_dimensions_thumbnail`,
  `png_sidecar_xmp_tags`, and the negative control
  `png_without_xmp_source_has_no_tags`; the JPEG APP1 scenarios were retained
  unchanged. Embedded PNG XMP stays unpromised: `pinned_png_compressed_embedded_xmp_is_not_extracted`
  pins that no decompression exists, and the manifest's contradiction rule was
  mutation-checked as guarding `png.xmp: ["sidecar"]`. The container-unaware
  byte scan is now documented on `extract_xmp_data`, including that compact XMP
  is not guaranteed detected and that an uncompressed PNG text chunk would be
  matched only by accident. Mutation checks: 6/6 new assertions detected.
  Contract questions recorded: the backend reads no IPTC anywhere (snapfab
  writes it, nothing consumes it — kept unclaimed, no parsing added), and a
  PNG-embedded-XMP API scenario is not feasible with the UTF-8-only `raw_file`
  fixture, so that claim is pinned at unit level. Gates: `cargo test -p picasu`
  (309), `cargo test -p snapfab` (33), `just backend-check`, `just utils-check`,
  `just plan-lint`, targeted Playwright metadata sidebar scenario — all pass.
- 2026-09-25 — Evaluated ffprobe as a second identifier for content and rejected
  it. Measured against ffmpeg 8.0: it reports a container group as a single name
  (`mov,mp4,m4a,3gp,3g2,mj2` for mp4/mov/m4v, `matroska,webm` for mkv/webm,
  `asf` for wmv), so it cannot distinguish what `infer` separates by brand and
  DocType, and it fingerprints nothing `infer` misses among the accepted
  formats. Detection stays with `infer` alone; ffprobe remains only where it
  already was, decoding video.
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
