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
