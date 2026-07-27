---
status: done
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

| ID  | Severity | Description                                   | Status |
| --- | -------- | --------------------------------------------- | ------ |
| P0  | Critical | Filename not sanitized — path traversal       | Open   |
| P1  | High     | Content-Type header trusted, not file content | Open   |
| P2  | Medium   | No `last_modified` bounds check               | Open   |
| P3  | Low      | `unreachable!()` panic path in rename loop    | Open   |
| P4  | Low      | Partial failure on multi-file upload          | Open   |

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
