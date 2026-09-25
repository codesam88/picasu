---
status: done
type: bug
priority: high
area: backend
---

`GET /get/get-rows` and `GET /get/get-scroll-bar` bind
`GuardResult<GuardTimestamp>` and then discard the result with `let _ = auth;`,
so both handlers run to a `200` even when the request carries no
`Authorization: Bearer <timestamp token>`, or an invalid or expired one. The
guard's timestamp-vs-claims check is skipped as well, so the `timestamp` query
parameter is never validated against the token.

The sibling `GET /get/get-data` propagates the same guard with
`guard_timestamp?` (`src/router/get/get_data.rs:48`) and additionally applies
`show_download` / `show_metadata` from the resolved share. No fairing or global
auth layer compensates; guards are per route.

Exposed without credentials:

- `/get/get-rows` — row layout of a snapshot: asset count and every asset's
  pixel dimensions. No filenames, paths, tags, EXIF, or content.
- `/get/get-scroll-bar` — the temporal distribution of the library
  (year/month buckets and their start offsets).

Reaching them requires a valid snapshot id, which is a millisecond epoch minted
by `next_snapshot_id()` during prefetch, so exploitation is not trivial. The
same path also reaches a robustness bug: an unknown snapshot id makes
`/get/get-scroll-bar` panic at `src/storage/cache.rs` (`.expect("failed to read
tree snapshot for scrollbar")`) on unauthenticated input, and
`/get/get-rows` return 500. The process survives the panic, but the input is
attacker-reachable without credentials.

Likely accidental. Commit `84f29aa5` ("refactor: update read-only mode handling
to use Result") rewrote both handlers from `_auth: GuardTimestamp` (which
enforced 401) to `GuardResult<GuardTimestamp>` plus `let _ = auth;`, while
converting sibling handlers in the same commit to `let _ = auth?;`. The frontend
already sends a bearer token for both routes
(`frontend/src/api/fetchScrollbar.ts`, `frontend/src/worker/toDataWorker.ts`),
so the intended contract is authenticated.

## Notes

Fix by restoring propagation (`let _ = auth?;` or `guard_timestamp?;`) in both
handlers, add a scenario that asserts `401` for a missing, malformed and
expired token, and document the `401` on both operations with
`(status = 401, response = Unauthorized)`. Then delete
`operations_that_cannot_return_401_declare_none` in
`backend/src/tests/openapi_contract.rs`, which currently pins the absence of the
401 and exists only because of this gap.

Decide separately whether `/get/get-scroll-bar` should still panic on an unknown
snapshot id, or return a client error.

## Progress

- 2026-09-25: **Decided: an unknown snapshot id returns `400`, never panics.**
  The Notes section's open question — should `/get/get-scroll-bar` still panic
  on an unknown snapshot id — is resolved as a client error on both
  `/get/get-scroll-bar` and `/get/get-rows`. Rationale: the snapshot id is
  client-supplied (a millisecond epoch handed out by `/get/prefetch` and
  dropped again by the ~1h expire check), so asking for an expired or
  never-minted id is ordinary invalid input, and both operations already
  publish `(status = 400, description = "Invalid input")`, so 400 matches the
  checked-in contract with no spec change. A panic on input-dependent control
  flow converts to an opaque 500 at best. Mechanism: `read_tree_snapshot` now
  returns the typed `SnapshotReadError` (`NotFound` when the id's redb table
  does not exist, `Storage` for `begin_read`/iteration/decode failures);
  `read_scrollbar` returns `Result` (all three former `.expect` sites
  propagated; an unconvertible stored date is a `Storage` data error); the
  handlers map `NotFound` → `ErrorKind::InvalidInput` (400) and `Storage` →
  `ErrorKind::Database` (500) via one shared `map_snapshot_read_error` in
  `get_data.rs`. Verified test-first: scenarios
  `unknown_snapshot_get_rows_400.yaml` / `unknown_snapshot_get_scroll_bar_400.yaml`
  (valid bearer token minted for a future-but-unknown id) failed before the
  change (both answered 500, the scrollbar one after panicking at
  `cache.rs:119`) and pass after; unit tests
  `read_scrollbar_unknown_timestamp_returns_not_found` /
  `read_row_unknown_timestamp_returns_not_found` assert the `NotFound`
  variant without `#[should_panic]`. Known follow-up outside this scope: the
  frontend swallows the new 400 (`workerAxiosInterceptor.ts` only toasts on
  500), so a stale tab gets silent empty rows — needs its own task.
- 2026-09-25: **Fixed.** Both handlers now propagate the guard with
  `let _ = auth?;`, matching the sibling `/get/get-data`; `get_scroll_bar`
  returns `AppResult<Json<Vec<ScrollBarData>>>` so the guard error responds
  401 while the success path keeps the same body and status. Both
  `#[utoipa::path]` annotations declare `(status = 401, response = Unauthorized)`,
  both operations were added to `GUARDED_OPERATIONS`, and the
  `operations_that_cannot_return_401_declare_none` tripwire was deleted.
  Verified test-first: two new scenarios
  (`backend/tests/scenarios/token_get_rows_requires_token.yaml`,
  `token_get_scroll_bar_requires_token.yaml`) assert 401 for missing, malformed
  and expired bearer tokens against a real prefetch snapshot id — both failed
  against the old handlers (200 vs expected 401) and pass after the fix. The
  scenario DSL gained a `mint_timestamp_token` when-item (harness
  `backend_api.rs` + `tests/schema.json`) because an expired token's `exp` is
  signed server-side and could not otherwise be produced. `cargo test --lib`,
  `cargo test --release --lib`, fmt/clippy and `just check` (incl.
  `openapi-check`) pass. Origin re-verified against git history: `84f29aa5`
  rewrote both handlers from `_auth: GuardTimestamp` to
  `GuardResult<GuardTimestamp>` plus `let _ = auth;` while converting sibling
  handlers in the same commit to `let _ = auth?;`. The separate panic on an
  unknown snapshot id for `/get/get-scroll-bar` remains an open decision,
  unchanged by this fix.
