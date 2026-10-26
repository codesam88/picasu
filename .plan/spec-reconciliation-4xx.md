---
status: done
type: chore
priority: high
area: testing
---

## Problem

The Schemathesis pilot's phase-one failures are ~36 framework statuses
the spec never documents: Rocket data-guard 422s (spec claims only
400, which never fires for these), undocumented 404/415 on
body-less/format-mismatch requests, and a wrong content-type on
get/config/export (spec says `text/plain`, server returns JSON).
Until reconciled, every fuzz run produces ~40 noise findings, which
blocks any failing gate. Precondition for the api-assurance step 4
gate decision.

## Goal

Each observed status is either documented (spec gains the status or a
`default` response) or normalized server-side (map to the documented
status). A clean single-op sweep over the seeded instance is the exit
test.

## Steps

1. Triage each pilot cluster (A/B/D/G) to document-vs-normalize.
2. Change annotations and/or handlers accordingly.
3. Re-run the sweep (pilot harness, scratch) to confirm near-zero noise.

## Resolution (2026-10-26)

Triage rule: 422 (Rocket data-guard, distinct from declared 400) and
404 (format/body-less guards, global normalization would fight the SPA
catch-all) documented per operation with precise descriptions; 415
normalized to 400 via a catcher (same condition as the declared 400);
`GET /get/config/export` spec corrected to `application/json` (the
handler always returned JSON). Excluded: the three filed real-bug
classes (SPA-fallback 2xx, unknown-snapshot 500, create_dir_album
EEXIST). openapi-sanity's status universe gained 422 for routes with
dynamic input, mirroring its existing early-400 rule.

Exit sweep (pinned 4.30.1, seeded instance): remaining failures are only
the filed-bug operations — `Undocumented status 1` (redirect-to-login
SPA fallback), `Undocumented content-type 3` (get-data/rows/scroll-bar
SPA fallback), `Server error 1` (unknown snapshot). No new status
classes. Gates: openapi-check green, fmt/clippy clean, 808 nextest.
