---
status: backlog
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
