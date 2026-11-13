---
status: backlog
type: feature
priority: medium
area: backend
---

## Notes

Upload and other long operations need first-class, user-specific
progress tracking instead of the current fire-and-forget POST plus a
spinner that cannot report anything.

### Decision (2026-11-13): upload is async, final

Store-and-return upload shipped (`f3703c17`, `763ca3b8`): the 200
response is the completion signal; indexing runs in the background.
The interim wait-for-indexed grid poll was removed from
`uploadStore` — completion toast fires on 200 and the grid refresh
picks up rows within the ~1 s view freshness window (decoupled view
invalidation). A prototype `GET /get/asset/exists` poll target was
discarded: nothing consumes it, and an arbitrary-path lookup route
needs share-subtree scoping to avoid an existence oracle — any future
status endpoint exposing per-file state must carry the same scoping
review. This ticket is now the sole mechanism for surfacing background
status (scan/indexing/move progress) and feeds
`error-handling-and-activity-log`.

## Problem (verified 2026-11-07 against `post_upload.rs` + `uploadStore`)

- `POST /post/upload` is fully synchronous: one multipart request with
  all files, response only after per-file write + full `index_image`
  pipeline + tree flush waits ("a 200 means the row is visible").
- The frontend flips to `Processing` (indeterminate) the moment the
  request body is sent, so the long visible phase is server-side work
  with no progress channel. The remaining-time subtitle computes
  exactly 0 in this phase — actively misleading.
- Nothing refreshes the grid on `Completed`; new rows are queryable but
  invisible until navigation. The mini-card never auto-dismisses.
- Net effect: confirm returns to the grid instantly, a spinner with no
  progress remains, and finished uploads don't appear — reads as
  background work, but nothing runs in the background.

## Proposal

1. **User-specific status log (server-side).** Every long operation
   (upload, album move, asset delete, …) registers an operation with
   id, type, state, per-item progress/results, and errors. The
   frontend polls or subscribes per user and renders progress plus a
   persistent, investigable record. This _feeds_ the client-side
   activity log UI in `error-handling-and-activity-log` (which stays
   the display layer); this ticket owns the server-side operation
   state.
2. **Upload UX split.** Keep multi-file upload with a single Cancel and
   honest progress. Separate two concerns: the upload spinner shows
   _work in progress that can still be cancelled_ (bytes sent, file
   i of N), while the server-side processing status tells _what was
   done_ (indexed, skipped, failed per file).
3. **Backend decoupling.** Upload only uploads (fast 200 after write +
   validation). When the watcher is enabled, indexing and status come
   from the async watcher-triggered scan, separate from the upload
   action — removing the upload/watcher double-index conflict surface.
   When the watcher is disabled, keep a synchronous index path so
   uploads still complete without it.

## Open questions

- One-by-one POST vs single multipart: per-file requests give real
  file-count progress with no backend change, but drop the batch
  all-or-nothing guarantee (one bad file aborts everything today).
  Product call.
- Cancel semantics against server-side work: cancel remaining files
  only, or also roll back committed ones?
- Progress state store (DB table vs memory), retention, per-user
  scoping and auth on the status endpoint.
- Interaction with `upload-remove-file-on-index-error` and the
  current 400-on-undecodable behavior once indexing is async.

## Separable interim (unblocked, no ticket needed)

- Frontend-only honest `Processing` label (elapsed + file count,
  replacing the bogus remaining-time subtitle) and grid refresh on
  `Completed`. Lands independently of the decoupling.

## Relations

- Feeds: `error-handling-and-activity-log` (display layer).
- Touches: `upload-conflict`, `upload-remove-file-on-index-error`,
  `openapi-upload-query-params`, `index-move-scan-race`,
  `watcher-remove-events`, `worker-pool-rework`.
