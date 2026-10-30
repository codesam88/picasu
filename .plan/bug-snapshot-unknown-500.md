---
status: done
type: bug
priority: medium
area: backend
---

## Problem

Edit ops `edit_flags` / `edit_rating` / `edit_tag` answer 500
(`Failed to open tree snapshot`) for an unknown snapshot id
(`timestamp=0` or any garbage) instead of 400/404. Found by the
Schemathesis pilot (cluster H); reproduced manually. The spec even
documents 500 for these ops, so the contract currently accepts a
crash-shaped answer for bad input.

`set_user_defined_description` fails the same way: it calls the same
`open_tree_snapshot_table(...)` with an unvalidated `timestamp`
(`backend/src/router/put/edit_description.rs:76`) and answers 500 on
`timestamp=0` (found by the fuzz check 2026-10-26). Every
snapshot-taking op is exposed, not only the three edit ops.

## Goal

Unknown snapshot id is rejected as client error (400/404); no 500 for
any `timestamp` value. Spec updated to match (drop documented 500 or
scope it to genuine storage failures).

## Steps

1. Pin the behavior: unknown `timestamp` on each affected op
   (`edit_flags`, `edit_rating`, `edit_tag`,
   `set_user_defined_description`) asserts non-5xx + JSON error body.
2. Fix at the snapshot-open path (validate before opening).
3. Re-run new tests + full backend suite; update spec annotations.

## Resolution (2026-10-30)

Fixed at the snapshot-open path, not per handler: `SnapshotReadError`
grew `into_app_error` (`NotFound` → 400 "Unknown snapshot id",
`Storage` → 500), `open_tree_snapshot_table` returns the typed error
instead of erasing it via `anyhow::Context`, and all five call sites
(`edit_flags`, `edit_rating`, `edit_tag`, `edit_description`,
`GET /get/get-data`) map through it — `get_data`'s local copy retired.
`get_rows`/`get_scroll_bar` already mapped this way. PUT handler docs
moved "unknown snapshot" from the 500 to the 400 clause, matching the
GET docs. Pinned by dispatch test `unknown_snapshot_is_400_not_500`.
