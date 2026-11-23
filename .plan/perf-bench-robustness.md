---
status: done
type: perf
priority: high
area: tooling
---

## Notes

Benchmark robustness work on `perf/new-baseline2`, based on
`origin/main`. The battery previously recorded during-scan numbers without
proving the scan was still running, used fixed repeat counts, and kept
several competing rendered reports. The final implementation makes each
measurement prove its background state, bases medians and intervals on the
raw measurements, and keeps one final output pair.

## Implemented

- **Scan guards.** `ScanPhase` snapshots state and counters around probe,
  storm, storm-idle and post-scan phases. Probe rounds are valid only when
  the scan is running at start and end; invalid rounds are warned and
  excluded. Concurrent storms accept a terminal end after a running start
  and use scan-wall coverage as their hard check.
- **Probe loop.** Probes run until the scan is terminal, not until the row
  count reaches the expected count. Probe rows are cached and status is
  refreshed before probe/storm threads start.
- **Warmup.** The first measured scan issues one prefetch/first-image/scroll
  sequence and discards the timings.
- **Adaptive repeats.** `BATTERY_REPEATS` is a cap (micro 5, gate/cell/scale
  3). Control/upload cells continue until `MIN_PROBE_ROUNDS` (default 20)
  valid rounds or the cap; storm cells use one rep. A single rep still emits
  a `<cell>:median` row.
- **Raw measurement pooling.** P50 medians and `scan_wall_s` pool every raw
  hyperfine run, fetch sample, storm op, upload file and scan wall across
  rounds and reps. `n_samples` is the raw count; `n_clusters` counts the
  source rounds/runs/files.
- **Confidence intervals.** Seeded percentile bootstrap over all raw values,
  with a resample cap for large storm samples. Reported inline after each
  median as `95% CI`; diagnostic only, never a gate.
- **Final output.** `report.tsv` is the machine-readable staging/evidence
  record. `baselines.tsv` and `baselines.md` are the only rendered outputs.
  No `report.md`, `matrix.tsv` or `confidence.tsv` is generated. JSON is
  written only on explicit request.
- **Baselines.** Nine columns: `cell metric baseline ci_low ci_high
n_samples n_clusters recorded note`. Values and intervals use one decimal;
  counts stay integers. Legacy five-column rows still load.
- **Storm matrix.** `storm-{read,thumb,edit}-{8,16n}`, where `16n = 16*NCPU`,
  plus `storm-idle-8/16n`; `storm-diff` remains a legacy alias for
  `storm-edit`. Concurrent storms sustain until the scan terminal state with
  `clients*ops` as a minimum, capped at 256 ops, and
  `STORM_MIN_COVERAGE_PCT` (default 50) as the guard. `storm_coverage_pct`
  is reported.
- **Scale parity.** Gate and scale share one scenario matrix and differ only
  in image count/repo.
- **Comparison.** `compare_reports.py` replaces `battery check`. It accepts
  report-to-report or long-format `baselines.tsv`, scopes baseline
  comparisons by level and accepted metric, and reports deltas, tolerance,
  history MAD and review flags. `bench compare`/`just bench compare` is the
  handler.
- **Tests.** Shape-only tests were pruned; parsing, math, rounding, CLI,
  fixture and error-path coverage remain.

## Current contracts

| Knob                     | Value                           |
| ------------------------ | ------------------------------- |
| `BATTERY_REPEATS`        | cap: micro 5, gate/cell/scale 3 |
| `MIN_PROBE_ROUNDS`       | 20                              |
| `MIN_VALID_ROUNDS`       | 3 per rep                       |
| `SCAN_WARMUP`            | 1                               |
| `PROBE_REFRESH_S`        | 0                               |
| `STORM_MIN_COVERAGE_PCT` | 50                              |
| concurrent storm minimum | `min(clients*ops, 256)`         |
| final rendering          | one decimal; counts integers    |

## Verification

- Micro and gate batteries ran with the final matrix and guards; warmup
  valid, probe rounds above target, sustained storms at 8 and 128 clients
  with zero failures and 86-91% coverage in the recorded runs.
- Baselines were re-recorded for 12 cells (micro 2 + gate 10).
- 410 unit tests pass; ruff and Prettier are clean; the pre-commit gate
  passed at the final tree.
- Branch history was squashed and the final tree preserved byte-for-byte.

## Follow-ups

- Scale is matrix-complete but not baselined; scale runs stay opt-in.
- CI covers p50 metrics and scan wall; p95/max have no intervals.
- Per-request hierarchical pooling is optional if raw iid intervals prove
  too narrow for a clustered metric.
