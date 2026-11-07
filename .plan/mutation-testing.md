---
status: idea
type: chore
priority: low
area: tooling
---

## Notes

Periodic (not PR-gating) mutation testing to catch vacuous tests that
pattern scans miss: delete/change production code, confirm the suite
fails. StrykerJS is the candidate for the frontend suite; equivalent
coverage for backend suites to be decided if this proves valuable.
Good to have, not now.

## Steps

1. Evaluate StrykerJS on the frontend suite (cost: runtime, config).
2. If valuable, schedule as a periodic audit and record the cadence.
3. Decide whether backend suites get the same treatment.

## Out of scope

The `vacuousTestBlocks` meta-test (shipped in
`frontend-structure-rework`) remains the cheap pattern-level backstop;
this item is the semantic backstop above it.
