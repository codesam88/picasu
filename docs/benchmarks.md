# Benchmark protocol

What the benchmark measures, how a measurement is defined, pooled,
validated and reported. This page is the contract: harness flags and
recipes live in `utils/bench/README.md`, and code that contradicts a rule
here is a bug in the code.

## Purpose

Detect performance regressions of the Picasu backend and attribute a change
to a cause — request path, scan stage, memory, ingest — before it reaches
users. A result is only useful if two runs of the same cell differ because
the backend differs, not because the harness selected a different sample,
statistic or execution path.

## Invariants

1. **One measurement model.** Every reported number is identified by cell,
   family, condition and statistic. There is no second naming scheme, no
   per-profile metric set and no column whose meaning depends on the
   profile that wrote it.
2. **One reducer.** Every statistic is computed from the pooled raw samples
   of its cell. Summaries of summaries are diagnostics, never reported
   values.
3. **One declared sample unit.** Each family states what one sample is. A
   metric's sample unit never changes between cells of the same family.
4. **Measured is accepted.** Every metric the harness reports is baselined
   and visible to comparison. "Informational" is not a category; if a number
   is not worth accepting, it is not reported.
5. **Proven background.** A sample counts only if its phase guard proves the
   background it claims. Validity is recorded per cell, not inferred from
   prose.
6. **Reproducible identity.** A cell records image count, client count,
   build and commit. Comparison refuses cells whose identity differs.
7. **Doc equals code.** A mechanism described here exists in the harness; a
   mechanism not described here is not part of the protocol.

## Measurement model

A cell is a point in a fixed space. The axes are independent, and a profile
selects which points it visits; a profile never changes what a point means.

| axis       | values                                                                   |
| ---------- | ------------------------------------------------------------------------ |
| images     | image count (50, 100, 200, 5650, 50k+) and file-size class (small/large) |
| background | `idle` (server at rest), `scan` (an album scan is running)               |
| access     | `prefetch`, `thumb`, `edit`, `scroll_bar`, `scroll_rows`, `scroll_data`  |
| profile    | the scenario, including a storm's client count                           |
| statistic  | `p50`, `p95`, `max`                                                      |
| repetition | scan (a populate scan inside a rep), rep (a whole cell invocation)       |

The upload cell's during-scan probes are `scan` samples like any other; the
previous `uscan` background named the cell, not the condition, and is gone.
A concurrent stimulus is not an access: it is the same access driven by a
storm profile, whose name carries the client count (`storm-read-8` is
`prefetch` at 8 clients). `storm-idle` is the same access load against the
tree at rest (`background = idle`). A cell records one concurrency, so a
storm cell does not also run the single-client probe phase.

A metric is a set of dimension values, never a name that encodes them:

```
(family, background, subject, stat) -> value
```

The statistic is a separate field, so `p50`, `p95` and `max` of one
condition are three rows differing only in `stat`. Rendered names are
generated from the dimensions; there is no second naming scheme.

## Metric families

| family  | subject             | background     | sample unit                | statistics    | interval |
| ------- | ------------------- | -------------- | -------------------------- | ------------- | -------- |
| latency | access              | `idle`, `scan` | one request                | p50, p95, max | p50 only |
| wall    | scan                | `scan`         | one scan                   | p50, p95, max | p50 only |
| stage   | stage name          | `scan`         | one scan's stage aggregate | p50, p95, max | p50 only |
| upload  | `submit`, `visible` | `idle`         | one uploaded file          | p50, p95, max | p50 only |
| gauge   | footprint name      | `idle`         | one rep's final value      | p50, p95, max | p50 only |
| count   | access              | `idle`, `scan` | one timed run              | sum           | none     |

The family fixes the statistics. A metric cannot opt out of p95 or max,
cannot add a private aggregation rule, and cannot be excluded from
baselines. `count` reports a sum because a count has no distribution.

## Aggregation

- **Pooling.** For one (cell, metric), every sample is pooled: probe rounds
  and scan runs inside a rep, and every rep of the cell. `p50` is the median
  of the pooled multiset, `p95` its 95th percentile (linear interpolation),
  `max` its maximum. One function implements each statistic.
- **Raw sources.** Probe accesses pool the individual request timings
  (fetch TSV rows, hyperfine `times`), not the per-round medians. Storm
  accesses pool the individual operation timings. Upload pools the
  individual files. Scan wall pools the individual scans.
- **Scan-level sources.** Server stage costs are reported by the observer as
  per-scan aggregates, so one scan's aggregate is one sample; p95 across
  scans is a quantile of scan-level aggregates. This is the one place where
  the sample unit is an aggregate, imposed by the source, and it is stated
  in the metric's unit.
- **Intervals.** The interval is a seeded percentile bootstrap over the
  pooled raw samples of the metric, reported for `p50` only. `n_samples` is
  the raw count; `n_clusters` is recorded as provenance — the number of
  rounds, scans, files or reps behind it — and does not change the method.
  A metric with no more than one sample has no interval.
- **Intervals are diagnostic.** They never gate a comparison by themselves;
  `compare` reports deltas and flags deviations for review.
- **Invalid samples are excluded.** A sample from a phase whose guard
  failed, or from a round that did not observe the claimed background, does
  not enter the pool, and the cell records the exclusion.

## Validity

Every cell records, next to its metrics:

- phase guard outcomes (probe, storm, storm-idle, post-scan): valid or not,
  with the observed start/end state and counters;
- probe rounds total and valid, and probe coverage (valid round wall over
  scan wall);
- storm coverage (active storm wall over scan wall) and ops done/planned;
- warmup validity;
- failed operation counts.

A rep with no valid round or an invalid guard is not pooled. A cell whose
recorded validity is false is not accepted as a baseline. The final outputs
carry the validity of the recording run, so a reader can tell a stable
number from a lucky one.

Warmup covers the single-client probe accesses on the first scan of a
repetition. Sustained concurrency is not warmed: a storm's ramp is part of
the load it claims to measure, and discarding a prefix of it would be an
arbitrary threshold.

## Scenarios

Profiles select the measurement phase; they do not change metric meaning.

| profile                         | phase plan                                                                                                            |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `control`                       | populate scans; probes and scroll during each scan; idle probes and scroll, then a final sweep, after it              |
| `storm-<access>-<clients>`      | one populate scan; sustained ops at the profile's client count until terminal, with a coverage floor                  |
| `storm-idle-<access>-<clients>` | one populate scan; one sustained burst at the same client count against the tree at rest, for a fixed op budget       |
| `upload`                        | settle the library; upload the cell's file set; populate scan over the enlarged row space; probes during and after it |

Concurrent storms outlast the scan because the question is how the request
path behaves under indexing load; an idle storm has no scan to outlast, so
its budget is a fixed op count. Both record done/planned ops. An idle storm
is one phase after the scan; storm profiles run one scan because they have no
probe-round budget to accumulate.

The upload cell's file set is part of the cell's declared stimulus: it
comes from `UPLOAD_REPO` when set, otherwise from the cell repo beyond the
staged prefix, and the cell fails when the source cannot provide that many
files. Re-uploading an image the cell already staged is not allowed: it
changes the row space the cell claims to measure.

## Image counts and matrix

The image count is the size parameter: `battery run --images N` runs the
full profile matrix against `set-N`. A cell id is `<profile>-<images>`.
There is no level above the image count.

The measurement budget, not a repeat count, is the control: a cell adds
scans until it has at least `MIN_PROBE_ROUNDS` (20) valid probe rounds, and
the scan cap (5 at 200 images or fewer, 3 above) only bounds cost. A large
image count usually satisfies the budget in one scan; a small one may need
several. Cells without a probe phase measure every operation of one
sustained run and need one scan. A cell that cannot reach the budget fails
instead of recording a thin sample.

Every image count runs the same profiles with the same pinned environment;
only the repo differs. A 5650-image run is opt-in for cost. The debug path
(`bench run cell`) takes an arbitrary profile and environment; its rows are
never recorded as baselines.

Client counts are resolved when the battery starts: `16n` becomes
`16 × NCPU` of the recording host and stays part of the profile name.

## Pipeline

```
prepare   archive → sandbox
launch    server + pinned env + identity
stimulate workload phases emit raw samples and guard events
validate  guards and coverage decide which samples count
reduce    one reducer: (cell, metric) → value, n_samples, n_clusters
report    report.tsv: long staging record of the cell set
baseline  baselines.tsv: accepted values + intervals + validity
compare   duration/delta review against a baseline or another report
render    baselines.md: tables derived only from baselines.tsv
```

Each stage has one contract. The workload never computes a reported
statistic; the reducer never reads a transcript to invent one.

## Outputs

`report.tsv` is the staging record: one long-format row per
`(cell, family, background, subject, stat)` with `value`,
`unit`, `n_samples`, `n_clusters`, plus `# cell:` metadata lines carrying
identity and validity. It is evidence, not a deliverable.

`baselines.tsv` is the accepted record: the same dimension columns for
accepted cells, one decimal for values and interval bounds,
integer counts, plus `ci_low`, `ci_high`, `recorded` and a note, and a
`# cell:` identity/validity line per cell. Every metric in the report is
accepted; there is no separate allow-list.

`baselines.md` is rendered from `baselines.tsv` only, including its cell
metadata. Tables name every
statistic `p50`, `p95`, `max`; the interval appears inline after `p50`.
`report.md`, `matrix.tsv` and `confidence.tsv` are not produced.

## Prohibited patterns

These have appeared in this harness and are rejected in review:

- a per-profile, per-metric or per-family branch in aggregation;
- a metric that is measured but not accepted, or rendered but not
  compared;
- two execution paths for one label (`storm-diff`/`storm-edit`, numbered
  and unnumbered storm names, `diff` remaps, duplicate scroll helpers);
- statistics computed from summaries (median of round medians, worst of
  run worsts) except where the source only provides aggregates, as stated
  for stage metrics;
- hand-maintained metric allow-lists;
- unrecorded identity, or comparison across different identities;
- a range column that duplicates p95/max;
- prose describing a gate, artifact or mechanism the code does not have.

## Open items

- Cross-host comparison: `16n` and tmpfs staging are host properties.
  Rows record CPU and client counts; comparing across hosts is not
  supported yet.
- Large image counts stay opt-in until the small-count baselines are stable.
- Intervals use a bootstrap over pooled raw samples. Whether a per-request
  hierarchical pooling adds information beyond that is open; the raw-sample
  interval is the current definition.
