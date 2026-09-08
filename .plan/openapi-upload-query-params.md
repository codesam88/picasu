---
status: open
type: feature
priority: low
area: backend
---

Document the `/upload` query parameters in the utoipa schema. Raised in PR \#17 review (2026-09-07); `openapi-backfill`
is closed, so this drift is untracked.

## Context

`#[utoipa::path]` on `backend/src/router/post/post_upload.rs:146-155` documents no query parameters at all
(`request_body = Value`, generic responses). The route signature declares three:
`presigned_album_id_opt`, `on_conflict`, and (new in PR \#17) `auto_rename`. `on_conflict` predates this PR
(`ef27af1d`), so the drift is pre-existing; the PR added one more undocumented, user-facing flag.

Per `openapi-backfill.md` (status done, "3 of ~65 routes annotated"), the spec is considered backfilled despite the
widespread absence of annotations — so nothing currently tracks these missing params.

## Tasks

- [ ] Add `params(...)` entries (or full `request_body` schema) for `presigned_album_id_opt`, `on_conflict`,
      `auto_rename` on the `/upload` route, including valid values (`on_conflict`: skip|rename|replace; `auto_rename`:
      bool) and the note that `auto_rename=false` rejects unsanitizable names.
- [ ] Decide whether to document just this route or widen the scope back to a general OpenAPI backfill pass; if the
      latter, reopen `openapi-backfill.md`.

## Progress (2026-09-08)

`auto_rename` done in PR \#17 (utoipa 5 `params(("auto_rename" = Option<bool>, Query, description = ...))`, confirmed
in a `--dump-openapi` run). `presigned_album_id_opt` and `on_conflict` remain undocumented — pre-existing drift, kept
out of PR scope.
