---
status: open
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
