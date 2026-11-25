---
status: open
type: feature
priority: medium
area: testing
---

## Problem

The model↔code bridge is the thinnest validation link: 25 hand-written gate
tests plus the OP-marker annotation layer. Model-based testing multiplies that
into generated scenario families replayed against the real implementation,
catching implementation drift as code evolves. Nothing exists yet: no trace
generation wired, no driver, no concretization mapping.

## Goal

A nightly/weekly MBT lane: Quint generates ITF traces, a Rust driver (built on
`quint-connect`, Driver trait + `#[quint_test]`/`#[quint_run]` macros) replays
them against the backend test harness, and the implementation state is diffed
against the spec state. Confidence in code↔spec agreement — not a proof. Merge
gates unchanged.

## Implementation steps

1. Depends on the nondeterministic `step` from the proof-lane task (shared
   prerequisite; do not duplicate).
2. Trace spike: `quint run --mbt --n-traces=N --out-itf` with fixed seeds;
   inspect the ITF format (`mbt::actionTaken`, `mbt::nondetPicks`); define the
   concretization mapping (model path/hash values → real tempdir files and
   bytes). Traces are generated artifacts, never committed — fold into the
   artifact-policy item in devops-cleanup.
3. Driver MVP on the feasible action subset only: direct API ops (upload /
   move / assign / sidecar edit). Excluded at first: sweep/worker actions
   (async seams), DB_LOSS (heavy seam), event-missed faults (not injectable —
   covered by rescan convergence instead).
4. Settled-checkpoint state comparison: quint-connect diffs spec vs driver
   state after every step, but the implementation converges asynchronously —
   `State::from_driver` must be evaluated at settled checkpoints (reuse the
   settle-wait patterns from existing scenario tests). This is the main
   engineering risk; resolve the design before scaling.
5. Lane: ~50 traces, fixed seed, nightly/weekly, not merge-gating.
6. Deferred: full action coverage, trace validation (production logs → model;
   acknowledged future work upstream).

## Decisions

- 2026-11-15: quint-connect chosen over hand-rolled ITF parsing (itf-rs) —
  maintained library (launched 2025-12, v0.1.x), per-step state diffing and
  macros built in. Brief: `docs/briefs/quint-modeling.md`.
- 2026-11-15: independent cheap lane registered in devops-cleanup instead:
  OpenAPI `links` for the stateful schemathesis phase (currently zero links in
  openapi.json) and custom `after_run` checks encoding the HTTP-observable
  read contracts (G3, API_READ_*).
