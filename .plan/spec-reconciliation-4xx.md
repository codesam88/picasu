---
status: open
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
