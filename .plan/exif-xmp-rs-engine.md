---
status: done
type: feature
priority: high
area: backend
---

# kamadak-exif + xmpkit: Rust-only metadata engine (`feat/exif-xmp-rs`)

## Goal

Read image metadata and write XMP sidecars entirely in-process with two Rust
crates — `kamadak-exif` for EXIF, `xmpkit` for XMP — removing the byte-scan XMP
reader and the hand-rolled sidecar writer. No write-back to image files, ever;
sidecars (`.xmp`, `.albuminfo.xmp`) are the only metadata this backend writes.
The backend stops needing `exiftool` at runtime. Snapfab keeps `exiftool` for
fixture generation. The branch also carries the format-capability package
(capabilities manifest, upload content-type validation, pinned metadata-contract
scenarios) ported from `feat/exif-xmp-handling` with its assertions rewritten to
the kamadak/xmpkit output contracts.

## Decisions (settled with the user, 2026-10-04)

1. **Engines:** `kamadak-exif` for EXIF read (already this branch's engine —
   no swap, no observable `exifVec` change), `xmpkit` 0.1.x for XMP read
   (embedded + sidecar) and XMP write (sidecars only).
2. **No image write-back.** The backend never writes EXIF/XMP into a media
   file. Only sidecar packets are written, atomically (temp file + rename).
3. **Snapfab keeps exiftool** — port the old branch's snapfab exiftool fixture
   writer (`4e7fa721`) over this branch's `little_exif` + `iptc` writers.
4. **`furtherMetadata` bucket: out of scope.** It stays absent; the API and
   frontend remain main-parity. A future plan can add it with a contract
   written for these engines (unmodelled EXIF tags + XMP properties).
5. **Format-capability package: in scope** (manifest, content-type validation,
   snapfab writer, fixture-contract scenarios, seeded randomization,
   scenario-harness/schema improvements) — ported with expected values
   rewritten to kamadak/xmpkit output.
6. **IPTC and PNG-text are not claimed.** Neither crate reads IIM or PNG text
   chunks, so the old branch's `iptc: [embedded]` claim, PNG-text mapping, and
   their scenarios are not portable (the manifest may record them as
   unsupported). Snapfab may still write such bytes into fixtures; nothing
   asserts them.
7. **Sidecar edits must preserve unmanaged properties.** The old branch's
   intent (`be191768`) is kept, implemented with xmpkit's XMP DOM instead of
   an exiftool read-modify-write. This is a behavior fix over current `main`,
   whose `format_xmp_packet` rebuilds a packet and drops anything unmanaged.

## Current state (measured on this branch, 2026-10-04)

- **EXIF read:** `process/exif.rs` via `kamadak-exif`, PRIMARY IFD only,
  `field.display_value()` strings (dash dates, orientation prose). `misc.rs`
  orientation/dimension matching already matches these strings — unchanged
  by this plan.
- **XMP read:** `process/xmp.rs::extract_xmp_data(bytes)` — hand-rolled byte
  scan of four managed fields (`dc:subject`, `dc:description`, `xmp:Rating`,
  `dc:title`). Container-unaware; compact XMP not matched; compressed PNG
  `iTXt` not read (9 unit tests pin the contract and its quirks). Entry seam:
  `extract_xmp_data_from_file(path)` — sidecar preferred, else file bytes;
  callers are `process/index.rs` (×2) and `process/dir_album.rs`.
- **XMP write:** `process/xmp_write.rs::write_sidecar_for` —
  `format_xmp_packet` builds a fresh packet from the four managed fields
  (unmanaged properties destroyed), atomic temp + rename, `io::Result`.
  Callers: `xmp_write.rs`, `dir_album.rs`, and the five edit endpoints
  (`edit_album`, `edit_description`, `edit_rating`, `edit_tag`). Sidecar
  failures are best-effort (`warn!`, the edit still commits).
- **Rebuild:** `rebuild.rs` clears and repopulates identity tables only;
  metadata cache is not re-read from raw files.
- **Snapfab:** `little_exif = "0.6"` + `iptc = "0.3"` fixture writers; no
  `capabilities.json` on this branch (`schema.json` exists; `scenario_schema.rs`
  does not).
- **Suite:** 108 scenarios, no format-manifest gates, no upload content-type
  validation, no `exiftool`/`ffmpeg` in CI (neither is needed yet).

## What we do NOT take from `feat/exif-xmp-handling`

Explicitly non-portable — record here so a later port does not rediscover it:

| Old-branch work                                                                                                                                                                      | Why not                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------- |
| Backend exiftool engine (`ba6162f5`) + toolchain hard-fail (`22d188f9`)                                                                                                              | The engine this plan replaces; in-process crates have no toolchain to fail |
| Missing-tool precondition tests, `install-exiftool` as a backend runtime need, exit hook (`c74b0597`)                                                                                | Backend spawns no exiftool children                                        |
| IPTC/PNG-text native-field mapping (`ba7de55`), claims (`5967fd85`), `jpeg_iptc_only_*` / `edit_tag_removing_an_embedded_iptc_*` / `clearing_a_description_*_iptc_caption` scenarios | Neither crate reads IIM or PNG text (decision 6)                           |
| `furtherMetadata` bucket (`ae7180a7`) + frontend component                                                                                                                           | Decision 4                                                                 |
| exiftool display-string contracts (colon dates, `Rotate 90 CW`, `ExifTool:Error` records)                                                                                            | This branch's contracts stay kamadak-shaped                                |
| Release-workflow tarball changes                                                                                                                                                     | Unrelated to the engine                                                    |

## Iteration 0 — Spike: settle both crates against our fixtures

Nothing merges until this measures. Numbers and verdicts go into Progress.

- **xmpkit (read):** against checked-in fixtures — JPEG embedded APP1 XMP,
  the compressed-PNG-`iTXt` fixture, TIFF/WebP embedded, a plain `.xmp`
  sidecar, a corrupt sidecar, compact/attribute-form XMP. Record which read,
  what the `XmpData` four fields come out as, and whether `XmpFile::from_bytes`
  accepts whole-image bytes (matters for the read seam).
- **xmpkit (write):** open a sidecar carrying foreign properties
  (`xmp:CreatorTool`, `Iptc4xmpCore:*`), set the four managed fields, write;
  verify foreign properties survive, the output re-reads through xmpkit itself,
  and the serialization form (plain packet vs `<?xpacket?>`) is stable across
  two writes. Verify new-sidecar creation and `.albuminfo.xmp` naming.
- **kamadak (baseline):** record current PRIMARY-IFD display strings for the
  pinned fixtures (dash dates, orientation prose) and the known WebP bare-TIFF
  quirk — these are this branch's existing `exifVec` contract, kept as-is.
- **Verdict rule:** a carrier xmpkit cannot read is _unclaimed, not a gap_
  (precedent: old plan decision 7) — drop the intended manifest claim and say
  so in Progress. A needed case that fails (sidecar round-trip, compressed
  `iTXt`) triggers a recorded fallback decision before Iteration 1 starts:
  keep the byte-scan for that one case, or amend the contract.
- **Gate:** Progress entries for every claim we intend to make; explicit
  go/no-go per iteration below.

## Iteration 1 — XMP read through xmpkit

- **Tests first:** port this branch's managed-field unit tests (bag / alt /
  integer extraction, missing-field, sidecar-preferred) against the xmpkit
  reader — they should pass unchanged in intent. Flip the byte-scan quirk pins
  per Iteration 0: compressed PNG `iTXt` becomes a positive read if measured
  ✓, an honest unsupported pin if not; the container-unaware documentation pin
  goes away with the scanner. Add the corrupt-sidecar pin: sidecar present but
  unreadable suppresses the file's own XMP (no fallback to embedded) — the
  pre-existing precedence contract, now through a real parser.
- **Implementation:** swap the internals of `extract_xmp_data_from_file`
  (keep the seam and `XmpData` shape; callers untouched) to xmpkit open of
  sidecar-or-file. Delete `extract_xmp_data` byte-scan and its tests once
  replaced. If Iteration 0 forced the bytes path, keep `XmpFile::from_bytes`
  for sidecar content and the documented fallback for the one carrier.
- **Gate:** full backend suite and all 108 scenarios green (they assert
  kamadak `exifVec` and sidecar-derived tags — no expectation changes from
  Iteration 1 alone); `just backend-check`.

## Iteration 2 — Sidecar write through xmpkit, preserving unmanaged properties

- **Tests first:** port `sidecar_edit_preserves_unmanaged_xmp` and
  `albuminfo_edit_preserves_unmanaged_xmp` from the old branch (adapted:
  assertion values in kamadak/xmpkit terms) — RED on this branch, whose fresh
  packet drops the foreign properties. Unit pins: managed field updated +
  foreign property survives; no sidecar → created; album `.albuminfo.xmp`
  custom-title rule (explicit-`None` never baked in — the freeze-bug comment
  contract); corrupt sidecar on the write path → replaced with a clean managed
  packet and reported as such (port the old refusal-replacement semantics with
  the xmpkit parse error as the refusal signal).
- **Implementation:** rewrite `write_sidecar_for` to open-edit-serialize via
  xmpkit inside the existing temp-file + rename; map xmpkit errors onto the
  current `io::Result` signature (no caller churn). Keep naming rules
  (`.xmp` beside the asset, `.albuminfo.xmp` in the album dir).
- **Gate:** full suite + the new preserve scenarios green.

## Iteration 3 — Transactional sidecar edits (port `sidecar_edit.rs`, `53cba9e2`)

- **Tests first:** port the `sidecar_write_failure_*` scenario family and its
  backend_api assertions, plus whatever harness step the old branch used to
  force a write failure (read the old implementation and bring the step with
  it; keep this branch's OpenAPI annotation style — `tag` + `401 response`,
  not the old `put, path=…` spelling). RED: on this branch the edit commits
  with a `warn!` after a failed sidecar write.
- **Implementation:** port `process/sidecar_edit.rs` (`EditedItem`,
  `commit_metadata_edits`) with the sidecar write bound to the xmpkit writer;
  route the five edit endpoints through it so a sidecar write failure returns
  before the payload row is inserted and before the commit (the branch's
  contract; the closure doing DB writes stays as it was ported).
- **Gate:** full suite + failure scenarios green; no edit endpoint writes a
  payload row when the sidecar write failed (scenario-asserted).

## Iteration 4 — Sidecar precedence and metadata reconstruction (ports `0f42f977`, `64e7cebb`)

- **Tests first:** port the old branch's precedence/reindex pins in adapted
  form — `sidecar presence overrides all embedded metadata`, corrupt-sidecar
  reindex behavior, and the rebuild family (`rebuild_reconstructs_metadata_from
_raw_and_sidecar`, `rebuild_drops_stale_metadata_payload`,
  `rebuild_is_idempotent_for_metadata`). This branch has no rebuild-metadata
  scenarios today, so these are new pins, not flips.
- **Implementation:** port the rebuild metadata reconstruction (walk re-reads
  raw files + sidecars through the Iteration 1 reader into `METADATA_TABLE`)
  and close any precedence gap the read swap exposes in index/reindex.
- **Gate:** full suite + rebuild family green.

## Iteration 5 — Format-capability package (ports `4a8ada54`, `e98750a4`, `d459e5e5`, fixture/harness/seed commits)

Split into three phases; each has its own gate.

**5a — Manifest + snapfab writer + fixture plumbing**

- Port `utils/snapfab/capabilities.json` (claims rewritten per Iteration 0 and
  decision 6: `xmp` claims only what measured ✓; `iptc` recorded unsupported;
  video keeps `container: [probe]` + sidecar), snapfab manifest validation
  (fixture digests verified in snapfab's tests), and the snapfab **exiftool
  writer** (`4e7fa721`): swap `little_exif`/`iptc` deps for `exiftool` +
  `tempfile`, rewrite `test_image.rs`. Port the binary-safe `fixture` given
  step + `schema.json` entries and `scenario_schema.rs` (old branch's final,
  post-`7a7721cd` state).
- CI: add the pinned `just install-exiftool` step to both jobs (snapfab tests
  now need it). No ffmpeg step yet.
- Gate: `cargo test -p snapfab`, manifest-consistency tests, `just utils-check`.

**5b — Upload content-type validation (port `d459e5e5`, `1ec3e490`)**

- Port `process/format.rs` (supported-format table) and
  `validate_upload_content` (first-512-byte sniff vs declared `Content-Type`
  extension), the AppConfig flag, its OpenAPI/config surface, and the
  misnamed-upload scenarios. Engine-independent: near-verbatim port except
  annotation style and config plumbing that changed on this branch.
- Gate: full suite; `just openapi-gen && just openapi-check` (the new config
  field changes the spec and the reference doc regenerates).

**5c — Fixture-contract, fallback and seed scenarios (ports `5c07da41`,
`6c310941`, `a93ac994`, `887efe53`, `ed4f3ec6`, `76e964e3`, harness fixes
`86d39fa0`/`010c9854`)**

- Port the per-format metadata-contract scenarios, corrupt/truncated fallback
  scenarios, seeded randomization (`seeds.json`, `selection.rs`,
  `PICASU_RANDOM_SEEDS`, `randomize` schema), UI cross-format smoke scenarios,
  and the harness fixes — then **rewrite every `expectedMetadata` and
  `exifVec` assertion to this branch's contracts** (dash dates, orientation
  prose, kamadak tag names; xmpkit-derived sidecar values). The IPTC-only
  fixture scenario is not ported (decision 6).
- CI: add the ffmpeg install step (the MP4/MOV scenarios need `ffprobe`;
  their video-duration pins are engine-independent).
- Gate: full backend suite, `cargo test -p snapfab`, schema validation of every
  scenario, `just frontend-test` for the smoke flows, CI green on the PR.

## Iteration 6 — Docs, cleanup, final gates

- **Docs:** `docs/metadata.md` engine/writer tables → kamadak (EXIF) + xmpkit
  (XMP read and sidecar write), exiftool demoted to snapfab/dev-fixture tool;
  `docs/linux.md` drops exiftool as a backend runtime requirement and states
  the snapfab/dev-test need; `docs/design.md`/README where they name the
  engine; regenerate `docs/openapi-reference.md` if Iteration 5b changed the
  spec.
- **Cleanup:** remove dead byte-scan code and any leftover exiftool references
  in backend paths (grep `exiftool` under `backend/src` — expect zero outside
  comments that describe snapfab); `cargo deny` sweep after the Cargo.toml
  changes (`xmpkit` in, `little_exif`/`iptc` out).
- **Final gates:** `just check`, `just test`, `just openapi-check`,
  `just plan-lint`, `just plan-format`; PR against `main`.
- Mark this plan `done` with the gate results in Progress.

## Port inventory (`feat/exif-xmp-handling` → this branch)

| Old-branch commit                                                                       | Verdict                              | Destination                     |
| --------------------------------------------------------------------------------------- | ------------------------------------ | ------------------------------- |
| `4a8ada54` `e98750a4` format capability manifest                                        | port                                 | 5a (claims rewritten)           |
| `d459e5e5` `1ec3e490` content-type validation                                           | port                                 | 5b                              |
| `4e7fa721` snapfab exiftool writer                                                      | port                                 | 5a                              |
| `5c07da41` `6c310941` `a93ac994` `887efe53` fixture/fallback scenarios                  | port + rewrite                       | 5c                              |
| `ed4f3ec6` seeded randomization, `76e964e3` UI smoke                                    | port                                 | 5c                              |
| `86d39fa0` then-block harness, `010c9854` port probe/reap, `7a7721cd` schema harness    | port                                 | with 5a/5c                      |
| `53cba9e2` sidecar_edit transactional edits                                             | port (writer swapped)                | 3                               |
| `be191768` preserve unmanaged xmp                                                       | port (xmpkit DOM)                    | 2                               |
| `0f42f977` sidecar overrides embedded                                                   | port + scenarios                     | 4                               |
| `64e7cebb` `5c933791` rebuild metadata reconstruction                                   | port                                 | 4                               |
| `ba6162f5` `22d188f9` exiftool engine + toolchain fail                                  | not ported                           | —                               |
| `ba7de55` `5967fd85` IPTC/PNG-text mapping + claims                                     | not ported (decision 6)              | —                               |
| `ae7180a7` furtherMetadata bucket                                                       | not ported (decision 4)              | —                               |
| `c74b0597` exit hook, `0a77944b` CI steps (as-is), `4d91e14e` `7a7721cd` conflict fixes | not ported / re-derived              | 5a/5c add needed CI steps fresh |
| `06963720` release-workflow tarball changes, `f6f5d593` tsbuildinfo ignore              | out of scope                         | —                               |
| `22d188f9`, plan-doc commits (`2a6d37e6`, `9f0036d8`, `ef215ff6`, …)                    | not ported (superseded by this plan) | —                               |

## Risks and open questions

- **xmpkit maturity** (0.1.6, published 2026-09-28, single maintainer, README
  claims broader formats than the released crate). Mitigation: Iteration 0 is a
  hard gate; the fallback rule (unclaimed, not a gap) keeps scope honest.
- **Serialization stability:** if xmpkit's sidecar output is unstable across
  versions, scenario pins on packet bytes would churn — pin semantic content
  (parsed properties), never packet bytes.
- **Fixture digests:** image-byte SHA-256 pins must not be touched by the
  writer swap (only snapfab regenerates fixtures; regeneration commands in
  `capabilities.json` change with the writer — 5a).
- **Scenario rewrite volume (5c)** is the largest single diff; split further
  per format if the change grows unwieldy.
- **OpenAPI:** only the 5b config flag changes the spec; nothing else touches
  routes.

## Notes

- 2026-10-04 — **Iteration 6 done (plan complete → status: done).**
  Docs repaired against the shipped tree: `metadata.md` (crate/engine/writer
  tables → kamadak-exif + xmpkit + ffprobe/ffmpeg; exiftool scoped to snapfab;
  little_exif/iptc rows dropped; sidecar-only write + precedence prose made
  true), `linux.md` (exiftool = dev/tests-only bullet; ffmpeg keeps runtime
  scope), `scenario-dsl.md` (rebuilt against schema+harness: `then:` not
  `assert:` for API, given/when/assertion vocabulary incl. inline `then:`,
  `randomize:`/seeds, `chmod` with `octal`, given-only `truncate_file`;
  false `build.rs`-validates-openapi claim removed; forms the harness lacks
  documented out), `test-strategy.md` (fixture/toolchain/randomization/
  OpenAPI-four-phase/gap-list truth-ups; unverifiable prefetch-race bullet
  dropped), `paste-shim/README` little_exif→exr fix. `docs/openapi-reference.md`
  regenerated — note: HEAD's committed reference did not reproduce from
  HEAD's own `openapi.json` even before this work (pre-existing staleness,
  5.6k lines), the regen corrects it. Cleanup: `.gitignore` tsbuildinfo hunk
  - `git rm --cached` of both tracked artifacts (f6f5d593, pulled into scope
    as gate hygiene); `deny.toml` quick-xml comment rewritten for the real
    provenance (xmpkit runtime, not little_exif dev) — **no deny entries
    needed**; reference sweep leaves only true hits (snapfab's writer, the
    install recipe, an `Iptc4xmpCore` namespace URI in a preserve-test packet).
    Worker gates: `just test` exit 0 (backend 442, snapfab 77, vitest 87,
    playwright 39), `just openapi-check` exit 0 (all four phases: sanity 63
    handlers, json-match, spectral 0 errors/53 baseline warnings, routes 61),
    `just backend-audit` exit 0, `just plan-lint` exit 0; `just check` failed
    only on this plan file's prettier (parent formats at closeout).
- 2026-10-04 — **Iterations 5c-1/2/3 done** (three workers, each reviewed;
  parent re-ran gates and closed two follow-ups). **5c-1**: inline `then:`
  body/file assertions were parsed-and-dropped on base (status-only) — ported
  from `86d39fa0` with its selftests (2 red first) + `serve_image_ok`/
  `image_home_path`; three pre-existing scenarios corrected exactly as the
  source commit did; snapfab `selection`/`seeds`/library + `randomize:`
  harness landed (compile-red → 77 snapfab / 417 suite green); CI gained
  `Install ffmpeg` in both jobs; `further_iptc` given-form removed from
  schema (mutation-teeth shown); `sidecar_read_failure_*` already green
  (It3 behavior); capabilities second test green after selection existed.
  **5c-2**: 25 scenarios ported (JPEG/PNG 3, TIFF/WebP 4, MP4/MOV 6,
  fallbacks 9 +1 cross-ref, randomized 3), all green first run with
  expectations **measured** pre-run (kamadak dash dates/names, ffprobe values
  kept — local 8.0.1 matches CI-validated set, JPEG EXIF byte order measured
  `MM\0*`); `rebuild_reconstructs_identity_but_not_metadata` not ported
  (superseded on the source branch itself by It4's family); when-verb
  decisions: given `truncate_file`+`patch_file` ported, when-`truncate_file`
  and non-boolean `wait_index` removed from schema (nothing dispatches them);
  no IPTC/furtherMetadata references remain (grep-verified). **5c-3**: 3 UI
  smoke flows + `010c9854` port-probe/reap infra ported (playwright 39 green,
  frontend-test 74→87 after follow-up, frontend-check exit 0);
  `edit-rating-via-sidebar` expectation measured-corrected to xmpkit's
  attribute-form `xmp:Rating="3"`. **Parent follow-ups after 5c-3**:
  retained `frontend/tests/paths.test.ts` from the branch (transient-run
  13/13 → now permanent, +13 tests); replaced the worker's local
  eslint-disable with the branch's faithful `eslint.config.mjs` fix
  (TS-aware `no-unused-vars` for the tests block) and removed the orphaned
  enable — eslint 0 problems. Carried to It6: docs (scenario-dsl/test-
  strategy updates for the harness verbs both source commits documented,
  metadata/linux/paste-shim sweeps, `just docs-openapi` regen), `.gitignore`
  tsbuildinfo hunk from `f6f5d593` (amendment: pulled into scope — every
  `frontend-check` run dirties the tracked artifact), `deny.toml` sweep,
  final gates.
- 2026-10-04 — **Iteration 5b done** (worker; parent re-ran gates and fixed
  the one environmental gap). Content-type validation ported from
  `d459e5e5`+`1ec3e490`: `process/format.rs` (table as single source of
  truth, ffprobe-rationale comment included), config flag + spec, upload
  always-rejects-unidentifiable / mismatch-only-flag, index treats mismatch
  as unrecognized (skip+log, `unsupported_skipped`), `is_valid_media_file`
  stays name-only; 8 source files verified byte-identical to the commit's
  post-state, `rebuild.rs` reconciled onto It4's shared-pipeline shape
  (disclosed), `m4v` extension added. Red evidence: 6 scenario reds against
  base (2 genuine index-behavior reds — base indexed PNG bytes under `.jpg`
  and scan-failed on garbage — 4 message-contract reds, honestly labeled;
  flag-off path already green, its predecessor scenario deleted per the
  commit's rename), compile-red via the manifest↔backend capabilities
  cross-check (first test only; second deferred to 5c for `snapfab::selection`).
  Spec: `validateUploadContent` description hunk → `just openapi-gen`;
  json-match green. Gates (parent-re-run): full suite **382** green,
  `just backend-check` exit 0, `just openapi-json-match` exit 0. Also fixed
  locally: `@stoplight/spectral-cli` was in `package.json` but absent from
  stale `node_modules` — ran `npm ci`; `just openapi-lint` now exit 0
  (0 errors, 53 baseline warnings).
- 2026-10-04 — **Iteration 5a done** (worker; parent re-ran every gate,
  verified claim set + SHAs + CI steps). Manifest
  (`utils/snapfab/capabilities.json`, 6 formats/7 fixtures) records exactly
  It0's claim set: `xmp:embedded` for JPEG/PLAIN-PNG/TIFF/WebP,
  `iptc:embedded` and `text:embedded` under unsupported (decision 6 ✓ no
  format claims IPTC), video `container:[probe]`+sidecar; compressed-iTXt
  PNG recorded in the `png-48x32-xmp-text` fixture's provenance prose. 7
  fixtures copied byte-exact from the branch (SHA-verified, parent
  spot-checked 2) + 1 created (`picasu-png-48x32-xmp-plain.png`,
  `f699ae00…`, generator script + reproducible exiftool steps checked in).
  Snapfab writer swapped to exiftool (deps: `little_exif`/`iptc` out,
  `exiftool`/`tempfile` in); red evidence: 3 semantic reds against the old
  writer (char-boundary clip, missing eXIf chunk, XMPToolkit absence), claim-
  rewrite reds against the branch-verbatim manifest (8), digest teeth via a
  mutated SHA; green: snapfab 62, `scenario_schema` 4 (teeth demonstrated by
  removing `apiChmod` → 5 scenario reds → reverted), full suite 361,
  `just utils-check` + `just backend-check` exit 0 (parent-re-run), CI YAML
  parses with `Install ExifTool` (pinned recipe + PATH lines) in **both**
  jobs; justfile gained `exiftool_version`/`install-exiftool`. Disclosed
  wiring: `tests/mod.rs` module line, `jsonschema` dev-dep,
  `snapfab/main.rs` lib-target switch. Carried forward: schema.json still
  documents a `further_iptc` given-form the harness does not dispatch (inert;
  5c decides); `backend/src/tests/capabilities.rs` cross-check from `4a8ada54`
  **not ported — decide at 5b**; stale prose in `docs/metadata.md`,
  `deny.toml`, `utils/paste-shim/README.md` → It6; CI ffmpeg step deferred
  to 5c; selection/seeds/library tests deferred to 5c.
- 2026-10-04 — **Iteration 4 done** (worker; parent re-ran gates, spot-checked
  pipeline wiring). Rebuild now shares the incremental indexer's pipeline:
  `process_media_info` extracted as the single entry (`process/index.rs`),
  `workflow::index_media_file` added, `tasks/actor/index.rs` switched to it,
  `rebuild.rs` clears `METADATA_TABLE` with the identity tables and routes
  every discovered file through the shared pipeline into
  `transitor::store_metadata_record`; the router's `sync_metadata_table`
  identity-only rewrite is gone (it would have clobbered payloads). Six
  scenarios ported: three rebuild scenarios RED first (width 0-vs-2, stale
  tags surviving), then green; two precedence pins (`corrupt_xmp_sidecar_*`,
  `an_external_partial_sidecar_*`) ran **already green before any change** —
  It1's seam already implements `0f42f977`'s contract on this base (no
  IPTC/PNG fall-through exists here) — ported as regression pins, no fake
  red. Adaptations all disclosed: `furtherMetadata`/`IPTC:*` assertions and
  `further_iptc` givens stripped (decisions 4/6), comments reworded off
  ExifTool/IIM. Counters (`metadataIndexed`/`metadataFailed`…) **not
  ported** — no ported scenario asserts them (rule followed); recoverability
  kept, reporting dropped; three branch unit tests unportable (exiftool
  seam) — reported, not improvised. `backend/openapi.json` regenerated
  anyway because the route doc-comment feeds the spec and
  `committed_artifact_is_up_to_date` enforces it (2 hunks, description/
  summary only). Gates (parent-re-run): full suite 357 green,
  `just backend-check` exit 0, `just openapi-json-match` exit 0.
- 2026-10-04 — **Iteration 3 done** (worker, scenario + unit red-green shown;
  parent verified restore ordering, `commit_with` seam, re-ran gates).
  `process/sidecar_edit.rs` ported: sidecars first (It2's xmpkit writer),
  failure → `Err` before any payload row/commit, per-item sidecar backups
  restored on a later item's failure (batch rollback), writer injectable for
  tests. The four edit endpoints now fail the request on sidecar-write
  failure instead of `warn!`-then-commit. Five `sidecar_write_failure_*`
  scenarios ported (injection = `chmod 0500` on the asset dir; temp+rename
  fails with EACCES; non-root only). Harness additions, disclosed:
  `chmod` when-verb + `assert_json_not_contains` in `backend_api.rs`, and
  the remembered-modes restore placed **before** `reset_backend_state()`
  (verified at `backend_api.rs:854-857`) so a mid-scenario panic cannot
  leave a read-only dir breaking later resets. Red evidence: 5 scenarios
  failed 200-vs-500 under the old flow + diagnostic probe showing the cache
  mutated; unit red = compile-red then 5/7 semantic failures against a
  base-semantics stub. Endpoints ended byte-identical to the branch's final
  files (its annotations already match this base's style). Deferred to 5c:
  the branch's `sidecar_read_failure_rolls_back_the_batch.yaml` (name
  outside the It3 family; behavior is unit-pinned by
  `a_sidecar_that_cannot_be_read_rolls_back_the_batch_written_before_it`).
  Gates (parent-re-run): full suite 349 green + integration binaries,
  `just backend-check` exit 0.
- 2026-10-04 — **Iteration 2 done** (worker, unit+scenario red-green shown;
  parent reviewed `load_sidecar`/title scoping and re-ran gates). Sidecar
  writes now go through xmpkit: `load_sidecar` = NotFound → `XmpMeta::new()`,
  non-UTF-8/parse `Err` → warn + replace with a clean managed packet, other io
  errors propagate; managed fields set (subject Bag replaced wholesale,
  description/rating set, `dc:title` `TitleEdit::Unmanaged` for assets —
  never touched — `Set(custom_title)` for albums, freeze-bug contract kept);
  `serialize_packet` through the existing temp+rename. Red drivers ran first:
  unit preservation (`xmpRights:Marked was dropped`) and both ported
  scenarios (`file.contains` on `Iptc4xmpCore:Location`/`photoshop:City`
  missing under the fresh-packet writer). Ported scenarios:
  `sidecar_edit_preserves_unmanaged_xmp`,
  `albuminfo_edit_preserves_unmanaged_xmp` — no harness change needed
  (`raw_file` given already exists on this base at
  `backend_api.rs:796`; verified). Truncated-sidecar pin records the
  measured behavior: parse-Ok keeps fully-written properties and the write
  repairs the file; a cut inside a tag → `Err` → replacement path. One
  additional pin (`photo_title_is_left_where_someone_else_put_it`, ported
  from the branch's test list) was added green, not red — it fails under the
  old fresh-packet writer by construction; disclosed by the worker. Gates
  (parent-re-run): full suite 337 green + integration binaries,
  `just backend-check` exit 0.
- 2026-10-04 — **Iteration 1 done** (worker, TDD red-green shown; parent
  reviewed diff, re-ran gates). XMP read now goes through xmpkit:
  `extract_xmp_data_from_file` = sidecar → `read_to_string` + `XmpMeta::parse`
  (any error → empty, embedded never a fallback) else `XmpFile::open` for the
  embedded packet; new `extract_xmp_data_from_packet(&[u8])` serves
  `dir_album::read_albuminfo` (doc comment updated to name malformed content).
  Byte-scan and its six helpers deleted. Normalization per It0 shapes (Bag
  flatten, Alt first element, subject comma-split, Rating
  `i32→u8::try_from().filter(≤5)` — the main-era clamp rule kept verbatim).
  Red drivers: compact/attribute packet, malformed → all-empty (byte-scan
  leaked partial), sidecar-wins-over-embedded (hand-built JPEG APP1 in-test),
  non-UTF-8 packet bytes; plus a compressed-PNG iTXt unsupported pin (built
  in-test, valid CRCs) with its own red. Spike harness deleted. Ported tests:
  9 → 15 in `process::xmp`. Gates: focused 24 green, full suite 331 green +
  integration binaries, `just backend-check` exit 0 (parent-re-run).
- 2026-10-04 — **Iteration 0 done** (worker spike, parent re-ran the harness
  and verified the crate-source root causes). Verdicts:
  - **Embedded XMP read works** for JPEG APP1, PNG uncompressed iTXt, TIFF,
    WebP via `XmpFile::open`/`from_bytes` (whole-image bytes only — packet
    bytes come back empty). **Compressed PNG iTXt fails** with xmpkit's own
    `Compressed XMP in PNG not yet supported` (`png.rs:271`) → **unclaimed**:
    no regression, main's byte-scan never read it either. IPTC/PNG-text
    unsupported (decision 6 unchanged).
  - **Packet (sidecar) I/O must use `XmpMeta::parse` + `serialize_packet`.**
    `XmpFile::open` on any `.xmp` returns no data: `scan_for_xmp_packet`
    (`files/file.rs:141-190`) looks for the end marker's first `?` (always the
    `<?` of the marker itself) and on failure advances past the packet start
    instead of scanning on — it can never match. `XmpFile::save` is forbidden:
    it errors _and_ leaves a 0-byte file. This amends the plan's "xmpkit open
    of sidecar-or-file" wording; embedded reads keep `XmpFile::open`.
  - **Write via `serialize_packet` verified**: unmanaged properties survive
    (oracle-checked `xmp:CreatorTool`, `Iptc4xmpCore:*`), managed fields
    update, output is `<?xpacket?>`-wrapped, byte-stable across writes and
    runs; pins semantic content, never packet bytes (output carries
    `XMPToolkit: xmpkit`).
  - **Corrupt sidecar**: truncated → `parse` returns Ok with all fields empty
    (never panics; sidecar-wins still suppresses embedded ✓); malformed XML →
    `Err`. It2's refusal signal = parse `Err`; the truncated case gets its own
    observed-behavior pin written test-first in It2.
  - **Shape contract for It1**: title/description/subject arrive as
    `Array([String])` or, in compact/attribute form, a plain `String`
    (subject as one comma-joined string); Rating arrives as `String`. The
    seam normalizes into `XmpData` (Bag flatten, Alt first element, subject
    comma-split, Rating through the existing i32→u8 ≤5 clamp rule).
  - **kamadak baseline (unchanged contract)**: dash dates
    (`2024-05-06 07:08:09`), orientation prose (`row 0 at top and column 0 at
left`), Make quoted; sub-IFD fields count PRIMARY; IPTC-only JPEG →
    `No Exif data found` → empty map (main's `if let Ok` swallow); WebP
    EXIF with a JPEG-style prefix → `Invalid TIFF byte order` → empty map.
  - **Dependency**: `xmpkit =0.1.6, default-features = false, features =
[jpeg, png, tiff, webp, mpeg4, mutli-thread]` (upstream typo kept) —
    drops `full-formats`' lopdf/AES/md-5 tree (lock +24 lines); full spike
    re-ran green under the slim set. It6's cargo-deny gate re-checks.
  - Claim set for 5a recorded: `xmp: embedded` = JPEG/PNG(uncompressed iTXt)/
    TIFF/WebP; sidecar ✓; compact ✓ with normalization. Spike harness
    `backend/tests/xmpkit_spike.rs` gets deleted at the end of It1.
- 2026-10-04 — Plan written on `feat/exif-xmp-rs` (base: `origin/main` +
  `2bfed14c`, `9c83906f`). Decisions recorded: furtherMetadata out, format
  package in, snapfab stays on exiftool, no IPTC/PNG-text claims. Iteration 0
  (xmpkit/kamadak spike) is the first work item; nothing merges before its
  verdicts are recorded here.
