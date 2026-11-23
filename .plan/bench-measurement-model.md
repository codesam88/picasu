---
status: done
type: perf
priority: high
area: tooling
---

## Problem

The harness reported one measurement under several statistic definitions,
kept a wide report matrix beside a long baseline file, measured many columns
it never accepted or compared, and carried execution aliases and
per-profile branches. `docs/benchmarks.md` described a gate and artifacts
that did not exist.

## Implemented

- **One model.** A metric is `(family, background, subject, stat)` inside a
  cell identified by profile and image count. Families: latency, wall,
  stage, upload, gauge, count. Statistics are p50/p95/max, except count
  which is a sum. Concurrency lives in the storm profile name, so a storm
  cell records only its storm access.
- **One reducer.** `metrics.py` pools every raw sample per metric and
  derives p50, p95 and max from that multiset. A seeded bootstrap over the
  same samples gives the p50 interval; no other statistic has one.
- **Long pipeline.** `report.tsv` and `baselines.tsv` share the dimension
  columns; baselines add interval, provenance and note fields plus per-cell
  identity/validity metadata. `baselines.md` is rendered from baselines
  alone, grouped per image count, labelled by profile, with `±` intervals
  and no raw sample counts.
- **Nothing measured is hidden.** Every metric in a report is recorded in
  baselines and visible to `compare`; there is no allow-list. Scroll,
  p95, footprint, all stages and failed counts are now first-class rows.
- **Raw sources.** Probe latencies pool individual requests (fetch rows,
  hyperfine `times`), storms pool individual ops, upload pools individual
  files, walls pool scans. Stage samples are per-scan observer aggregates,
  the one source-imposed unit, stated in the contract.
- **Execution.** Named profiles only (`storm-<access>-<clients>`,
  `storm-idle-<access>-<clients>`); no `uscan` background, `storm-diff`,
  unnumbered aliases or `IDLE_PROFILE`. The image count is the only size
  parameter (`battery run --images N`); there is no level. Scans repeat on
  the probe-round budget; profiles without probes run one scan. Idle storms
  run once after the scan.
- **Upload stimulus.** `UPLOAD_FILES` defaults to 20 and comes from
  `UPLOAD_REPO` (battery cells pin `set-200`), or from the cell repo beyond
  the staged prefix. A source that cannot provide the set fails the cell;
  an indexed image is never re-ingested.
- **Comparison.** `compare_reports` is long-only, identity-guarded, and
  uses unit-based tolerances; it is a review tool unless `--strict`.
- **Tests.** 147 shape/alias tests removed; parsing, pooling, rounding,
  resume, render and compare behaviour remain. 271 tests pass.

## Verification

- Fresh batteries at 50, 200 and 100 images on `edbf1d53`, one scan per
  storm cell, probe budgets met (50-image control used two reps).
- 30 cells, 1698 baseline rows; each report self-compares against the
  baselines with 566 rows compared, 0 moved, 0 refused, the other counts
  absent from that report as expected.
- Suite, ruff (check and format) and Prettier green.

## Follow-ups

- Matrix gaps: upload under a scan background; storm thumb/edit under an
  idle background. The report states the upload gap; the storm gaps are
  visible as missing rows.
- Instrument: probe prefetch/edit are hyperfine-timed while probe thumb and
  all storm ops are in-process. Unify on in-process per-request timing so a
  foreground is comparable across backgrounds.

- Larger image counts remain opt-in; the upload source (`set-upload`) is
  shared by every count.
- Probe coverage on the gate control cell is ~29%; worth understanding
  before treating it as a quality gate.
- Stage metrics can report a real 0 (for example `compute_locate` with
  work but no measurable cost); the contract currently keeps that value.
