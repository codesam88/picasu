---
status: in-progress
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1d.

## Context

`post_upload` renames from tmp to final path with no conflict check —
`fs::rename` overwrites silently if the file already exists.

The on_conflict parameter was already added to the upload endpoint code
but has zero test coverage. The YAML DSL also didn't support multipart
upload calls — this was the only blocker.

## Implementation

### DSL changes

Add `upload:` as a new `when:` verb in the API scenario interpreter:

```yaml
when:
  - upload:
      file: /path/to/source.jpg # path relative to IMAGE_HOME (from given: photo:)
      filename: photo.jpg # multipart filename (default: basename of file)
      target_album: "${album}" # optional album ID
      on_conflict: skip # optional: skip/rename/replace
      last_modified: 1700000000000 # optional (default: now)
```

The interpreter:

1. Reads the file from disk (placed by `given: photo:`)
2. Constructs multipart form-data body
3. POSTs to `/upload` with query params for `presigned_album_id_opt` and `on_conflict`
4. Returns the response for standard `then:` assertions

### Schema

- Add `apiUpload` definition to `backend/tests/schema.json`
- Update `when:` to accept either `apiCall` or `apiUpload`

## Tasks

- [x] on_conflict parameter already in upload endpoint
- [x] Add `upload:` verb to DSL interpreter (`execute_upload`)
- [x] Add `apiUpload` to schema.json
- [x] Update `when:` dispatch to route to `execute_call` or `execute_upload`
- [x] Create test scenario: basic upload success
- [x] Create test scenarios: on_conflict=skip/rename/replace
- [x] Create test scenario: upload without target_album
- [x] Create test scenario: upload without auth (401 when password set)
- [x] Create test scenario: upload with bad content-type (400)
- [x] Fix reset_backend_state to clear password between scenarios
- [x] Add `then` field to apiUpload schema for per-item assertions

## Tasks — Filename sanitization + auto_rename

- [x] Sanitizer: move to `process/sanitize.rs`, tiers 0/1/2 + NFC flag
- [x] Config: `normalize_upload_filenames` (AppConfig, Toml, update handler)
- [x] Route: `auto_rename` query param, reject + generated fallback
- [x] DSL: `auto_rename` in schema.json + execute_upload + scenarios

### Extension handling note (2026-08-07)

`TempFile::name()` in Rocket 0.5.1 returns a _sanitized_ name — it strips
the extension and platform-forbidden characters (see `fs/file_name.rs`,
`FileName::as_str`). The old code relied on this for both safety and naming
(`photo.jpg` → `photo` → on-disk `photo.jpeg` via Content-Type extension).

We switched `get_filename` to `raw_name().dangerous_unsafe_unsanitized_raw()`
so the tier sanitizer sees the true client filename (Rocket's stripping
defeated `auto_rename=false` rejection for `<`, `>`, `/` etc.). The extension
strip is now done in `resolve_filename` via `Path::file_stem()`, preserving
the pre-existing "stored extension comes from Content-Type" behaviour
(`photo.jpg` → `photocat.jpeg`).

## Security Review (2026-07-27)

Code reviewed: `backend/src/router/post/post_upload.rs`, `backend/src/router/auth.rs`,
`backend/src/constant.rs`, `backend/src/router/put/edit_config.rs`.

Goal: assess upload feature robustness for upcoming multi-user support.

### Identified Issues

**P0 — Path traversal via unsanitized filename**

`get_filename()` reads the multipart `Content-Disposition` filename with no
sanitization. That filename is joined directly into `target_dir`:

```
let tmp_path = target_dir.join(format!("{filename}-{unique_id}.tmp"));
```

A client sending `filename="../../etc/cron.d/backdoor"` writes outside the
intended directory. The `create_dir_album` handler already guards against
this (`name.contains('/') || name.contains('\\')`) — upload does not.

**P1 — Content-Type spoofing**

Extension validation at `post_upload.rs:147` checks the extension derived
from the client-provided `Content-Type` header, not from file magic bytes.
A client lying about Content-Type can save arbitrary file content with a
benign extension.

**P2 — No timestamp validation**

Client-provided `last_modified` is applied without bounds checking.
Timestamps of `0` or far-future values can break display sorting and
file watcher indexing decisions.

**P3 — `unreachable!()` panic in `find_unique_upload_path`**

The rename loop runs to `u32::MAX` then panics. A bounded retry with
a clear error would be more defensive in production.

**P4 — Partial failure on multi-file uploads**

If request contains N files and file K fails validation, files 1..K-1
are already written and indexed. Error response returns early, leaving
upload partially applied.

### Overall Issues List

| ID  | Severity | Description                                   | Status      |
| --- | -------- | --------------------------------------------- | ----------- |
| P0  | Critical | Filename not sanitized — path traversal       | In progress |
| P1  | High     | Content-Type header trusted, not file content | Done        |
| P2  | Medium   | No `last_modified` bounds check               | Open        |
| P3  | Low      | `unreachable!()` panic path in rename loop    | Open        |
| P4  | Low      | Partial failure on multi-file upload          | Open        |

### Planned Remediation

P0 is a hard blocker for multi-user. Fix: sanitize filename before path
join — strip path separators, reject `.`, `..`, and null bytes. Consistent
with `create_dir_album` validation.

P1 is acceptable for trusted clients but should be addressed before
multi-user. Options: (a) sniff file content via magic bytes, (b) accept
the trust boundary and document it. For a self-hosted gallery the
client is generally trusted.

P2, P3, P4 are robustness improvements. For timestamp, symlink, and
max file size checks: add sanitization that can be optionally disabled
via config (matching the existing `read_only_mode` pattern). Note:
upload file size is already enforced by Rocket's `max_upload_size`
config (`builder.rs:71-78`, default `100MiB`) — no additional work needed
there.

## Filename Sanitization Design (2026-08-07)

Scope: P0 remediation plus robustness. Design decisions from review with
user; implementation is TDD, one issue at a time, commit per issue.

### Sanitization tiers (all ON by default)

| Tier | Rule                                             | Examples                                                                 |
| ---- | ------------------------------------------------ | ------------------------------------------------------------------------ |
| 0    | Directory separators, null byte; reject `.`/`..` | `/`, `\`, `\0`                                                           |
| 1    | Windows-forbidden chars + reserved names         | `< > : " \| ? *`, `CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`         |
| 2    | Unicode landmines                                | noncharacters (U+FDD0–U+FDEF, U+FFFE, U+FFFF), zero-width/bidi overrides |
| NFC  | Normalize composed form                          | macOS NFD → NFC dedup                                                    |

- Tiers 0–2 are **always on** — not client-toggleable.
- **NFC normalization is the only optional check**: config flag, enabled by
  default (`normalize_upload_filenames`).
- Sanitization logic lives in **one place**: backend `process/sanitize.rs`
  (existing `sanitize_tag`/`sanitize_text` pattern). The frontend does NOT
  model the rules — it only passes `auto_rename` and renders the server's
  static description.

### `auto_rename` query param on `POST /upload`

- `auto_rename=true` (default): sanitize-and-proceed. Tiers 0–2 strips
  applied; if the result degrades to empty/`.`, fall back to a generated
  `upload-{uuid}.{ext}`.
- `auto_rename=false`: if the filename needs _any_ sanitization → `400`
  with a message naming the file and the offending characters. Tier 0 is
  never disabled — `../` is never written raw even with `false`.
- Backwards compatible: absent param behaves as `true`.

### Frontend

- [x] New pre-upload options dialog (flow: pick files → dialog → confirm →
      POST). Currently no options dialog exists — upload is instant from a
      hidden file input (`uploadStore.ts:43` `triggerFileInput`).
- [x] Dialog has the `auto_rename` toggle (default ON) and an info icon →
      `v-tooltip` popup (existing pattern, `LinksPage.vue:33`) listing the
      applied rules as static copy.
- [x] `uploadStore.fileUpload(files, albumId)` signature extends to carry
      `autoRename`; appended as query param to the POST URL.
- [x] On `auto_rename=false` reject: server 400 message surfaces via the
      existing `errorDisplay` path (`uploadStore.ts:116`).

Implementation notes (2026-08-07):

- `uploadStore` gains `pendingFiles` / `pendingAlbumId` / `autoRename`
  state; `prepareUpload` stages files and opens the dialog; `confirmUpload`
  closes it and calls `fileUpload(files, albumId, autoRename)`;
  `cancelUploadOptions` discards. Both `triggerFileInput` (file picker) and
  `DropZoneModal` (drag-drop) route through `prepareUpload`.
- New `buildUploadUrl(albumId, autoRename)` pure helper is unit-tested
  (`uploadStore.test.ts`); component rendering deferred to E2E per
  `docs/test-strategy.md`.
- New `UploadOptionsModal.vue` (BaseModal) renders the pending file list,
  the auto-rename switch with static rule copy in a `v-tooltip`, and
  Cancel/Upload actions. Registered in `App.vue`; `showUploadOptionsModal`
  added to `modalStore` dialog keys.

### Frontend E2E tests (2026-08-07)

Component rendering + the options dialog flow are tested via Playwright
scenario DSL (`frontend/tests/playwright/scenarios/*.yaml`), per
`docs/test-strategy.md`. No browser-side upload scenario existed before —
the DSL had no way to drive the hidden file input.

#### DSL additions needed

1. `given: source_file` — write a source file to a location OUTSIDE
   `IMAGE_HOME` (so the file is not indexed before upload) and record its
   absolute path in `ctx.vars`. Mirrors `photo_raw` (same flat shape, plus
   `id_as`):

   ```yaml
   given:
     - source_file: "bad:name.jpg" # literal filename, may contain unsafe chars
       format: jpeg
       width: 64
       height: 64
       id_as: $src_unsafe
   ```

   Implementation: snapfab batch with `output = {DIR}/source/{name}`,
   `vars[$id_as] = abs path`. Do NOT add to `seedEntries` (no index).

2. `when: upload.files` — click the upload trigger and set files via the
   Playwright filechooser (the input is created dynamically by
   `triggerFileInput`, so we cannot `setInputFiles` on a stable locator):

   ```yaml
   when:
     - upload.files:
         trigger: icon/mdi-upload # GalleryBar upload button (level-1 routes)
         files: ["${src_unsafe}"] # var refs / abs paths, interpolated (${name} form)
   ```

   Implementation: `const [chooser] = await Promise.all([
page.waitForEvent('filechooser'), clickTrigger() ])` then
   `chooser.setFiles(paths)`.

3. `when: set.auto_rename` — set the dialog switch to a specific state. Read
   the current state from the switch input (`#upload-options-modal
input[type="checkbox"]`, the only checkbox in the dialog) and click the
   `data-testid="upload-auto-rename"` list item only if it differs:

   ```yaml
   when:
     - set.auto_rename: false
   ```

4. `ui.toast`, `ui.modal`, `ui.text_visible`, `ui.count` already cover the
   assertions. Success = `Files uploaded successfully`; reject 400 body
   names the file + "Set auto_rename=true to allow the server to rename it."

#### Scenarios

| File                                               | Behavior                                                                                                           |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `upload-options-dialog-default-sanitizes.yaml`     | pick unsafe-named file, keep auto_rename ON (default), Upload → success toast; dialog lists the file before upload |
| `upload-options-auto-rename-off-rejects.yaml`      | pick unsafe-named file, `set.auto_rename: false`, Upload → error toast naming the file                             |
| `upload-options-auto-rename-off-safe-accepts.yaml` | pick safe-named file, `set.auto_rename: false`, Upload → success toast                                             |
| `upload-options-cancel-discards.yaml`              | pick file, Cancel → dialog closes, no upload (no success toast, gallery count unchanged)                           |
| `upload-options-multi-file-listed.yaml`            | pick 2 files, both listed in dialog, Upload → success                                                              |

All scenarios seed one indexed photo (`given: photo: seed/img01.jpg`) so
the timeline grid renders and the GalleryBar `mdi-upload` button is the
trigger (avoids the empty-state onboarding dialog). Post-upload file-name
verification stays in the backend DSL (predictable names via
`on_conflict: replace`); here the success/error toast is the signal.

### Test Coverage Gaps

The 10 existing scenarios cover happy paths, auth, conflict strategies,
and basic error responses. Missing security cases:

- Path traversal via malicious filename
- Special characters in filename
- Multi-file upload
- Concurrent upload (TOCTOU on `find_unique_upload_path`)
- `last_modified` edge values
- File exists on disk after `upload_dup_no_conflict` (currently only
  asserts status 200, not that two distinct files exist)

## Progress (2026-08-08)

- Frontend E2E upload-options scenarios implemented and passing (24
  Playwright scenarios total, 5 new). DSL gained `given: source_file`,
  `when: upload.files`, `when: set.auto_rename`; `interpreter.spec.ts`
  traces browser-originated backend API calls so `covers.api` matches
  `POST /upload`. `just check` + `just test` green. Committed on
  `feat/pre01` and pushed (`ca0e67e1`, `7e9db125`, `7218f4e6`,
  `36eba57a`).
- Remaining plan scope: P1 (Content-Type spoofing), P2 (last_modified
  bounds), P3 (unreachable! in rename loop), P4 (partial multi-file
  failure) are still open.

## Progress — P1 Content-Type spoofing (2026-08-08)

Decision (user): add an **optional, default-enabled** backend gate that
cross-checks claimed Content-Type against file content; keep spoofed but
whitelisted media accepted (served with the whitelisted type); include
nosniff as defense-in-depth. Do **not** derive naming from content.

### Gate (`validate_upload_content`, default true)

- Config flag `validate_upload_content` threaded through `AppConfig`,
  `TomlGallery`, `AppConfig::from(TomlFile)` (+ `toml_round_trip_full`
  test), `PUT /put/config` (`edit_config.rs`), `GET /get/config`
  (`ConfigResponse`), test helper `write_config`, and reset in
  `reset_backend_state`.
- Implementation in `post_upload.rs`: reads the first 512 bytes of the
  temp upload and detects the format with the **`infer` crate** (magic-byte
  database, no runtime system dependency — `tree_magic_mini` was rejected
  because it loads the host's shared-MIME DB at runtime; hand-rolled video
  magic bytes were replaced with `infer` on review). Signature-based, never
  a full decode, so unusual-but-valid variants still pass.
- Family mapping (whitelisted extension → accepted signatures):
  - `jpg|jpeg|jfif|jpe` → `infer` `jpg`
  - `tif|tiff` → `tif`
  - `mp4|mov|m4v` → ISO BMFF (`mp4`/`mov`/`m4v`)
  - `mkv|webm` → EBML (`mkv`/`webm`)
  - `mpeg` → `mpg` (infer's canonical spelling)
  - everything else 1:1 (`png`, `webp`, `bmp`, `gif`, `avi`, `flv`, `wmv`)
- Mismatch → `400 InvalidInput` `"Uploaded content is {detected}, but the
declared type is {extension}"`; unrecognizable bytes → `400`
  `"Uploaded content is not recognized as {extension}"`.
- Stored extension is still derived from the declared `Content-Type`
  (`get_extension`) — unchanged.

### Serving-side defense-in-depth

- `get_img.rs`: `CompressedFileResponse` no longer derives `Responder`;
  manual `Responder` pins `Content-Type: video/mp4` on the `SeekStream`
  (mp4) arm, replacing `rocket_seek_stream`'s byte-sniffed MIME. `NamedFile`
  arms already set `Content-Type` from the file extension.
- `builder.rs`: `.attach(Shield::default())` → `X-Content-Type-Options:
nosniff` (plus X-Frame-Options, Permissions-Policy).

### Test scenarios (new)

| File                                          | Behavior                                                                                           |
| --------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `upload_content_type_spoof_rejected.yaml`     | PNG bytes declared `image/jpeg` → 400, message names png/jpeg                                      |
| `upload_content_type_garbage_rejected.yaml`   | text bytes declared `image/jpeg` → 400 "not recognized"                                            |
| `upload_content_type_validation_opt_out.yaml` | `given config validate_upload_content: false` → same spoof accepted (200)                          |
| `upload_unindexable_removed.yaml`             | gate off + garbage → 400 "could not be decoded" (was 500) AND the orphan file is removed from disk |

The last scenario also fixes a latent bug: unindexable bytes used to be
saved to disk and only fail later at index time, leaving an orphan file
and returning 500. Now `post_upload.rs` removes the file and returns 400.

**Deletion boundary (user decision, 2026-08-08):** the removal above is
only legal because it happens _within_ the failed upload request — the
upload never succeeded, nothing is "committed", and the user gets the 400
directly. Files must never be deleted after a successful upload; from that
point on, only an explicit user deletion action may remove a file.

DSL: `given config` handler extended with `validate_upload_content`;
`givenConfig` schema entries added (`validate_upload_content`,
`fs_notify_watcher`).

Verification: `just check` and `just test` both green (backend 164,
frontend vitest 12+12).
