---
status: done
type: bug
priority: medium
area: testing
---

## Notes

The duplicate-file UI scenarios (`duplicate-files-*`, `duplicate-refresh-*`,
`duplicate-move-*`) fail intermittently under parallel Playwright load and
always pass serially. Observed four times 2026-10-25/26 during full-suite
runs (`--workers=4`), each time a different member of the family, each time
green on isolated rerun and on a serial run of all six.

Failure signature: `ui.count .desktop-small-image` expects 3, receives 2
(the locator resolves 0 elements, then 2, and never 3 within the 15 s
timeout) — one seeded/uploaded item missing from the grid, apparently not a
slow render but an absent item. Related sightings include
"identical uploaded files remain independently visible after refresh" and
"duplicate files can be moved to album independently".

Next: reproduce deterministically (repeat full runs at `--workers=4` or
higher; capture the scenario's backend log + coverage JSON for a failing
run), then check the upload→inline-index path under contention and whether
the given-phase snapfab generation or a filename conflict silently drops one
upload. Not reproducible serially as of 2026-10-26.

## Resolution (2026-10-26)

Root cause: the upload path only _scheduled_ its write chain — the index
stage enqueued `FlushTreeTask` detached, and `flush_tree_task` in turn
enqueued `UpdateTreeTask` (the tree rebuild + query-cache version bump)
detached — while the grid reads exactly once after navigation. Under
parallel load a flush lost the race and the UI never re-reads, so the
count stayed low permanently. Files on disk were always present (the
captured failure had 3/3), and no upload or index error was logged.

Fix: the upload handler now awaits both hops before returning (empty
flush-wait orders after the pending insert on the per-type queue, then
the tree-update wait) — the same discipline `delete.rs` already uses, so
a 200 means the row is visible.

Verification: duplicate family 6/6 × 6 consecutive runs (pre-fix the
failure appeared within 2), full suite 50/50 × 2, backend 808/808 (no
wait deadlock).
