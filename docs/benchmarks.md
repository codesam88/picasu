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

The benchmark is a matrix: each foreground access is timed under each
background that exercises the server differently.

| axis       | values                                                                                      |
| ---------- | ------------------------------------------------------------------------------------------- |
| images     | image count — the sole size parameter (matrix: 50, 100, 200; larger counts opt-in for cost) |
| background | `idle` (no scan, server settled), `scan` (an album scan is running)                         |
| foreground | `prefetch` (read), `edit`, `thumb`, `scroll`, `upload`, `storm`                             |
| statistic  | `p50`, `p95`, `max`                                                                         |
| repetition | scan (a populate scan inside a rep), rep (a whole cell invocation)                          |

`storm` is a foreground load family, not a background: it names the
operation (read, thumb, edit) and the client count (`8`, `16n`). `upload`
is a foreground as well; a cell that uploads while another upload runs is
self-referential and is not executed. The target matrix is:

| foreground             | idle            | scan                   |
| ---------------------- | --------------- | ---------------------- |
| read                   | baseline        | during indexing        |
| edit                   | baseline        | during indexing        |
| thumb                  | baseline        | during indexing        |
| scroll                 | baseline        | during indexing        |
| upload                 | ingest at rest  | ingest during a scan   |
| storm-<kind>-<clients> | loaded baseline | loaded during indexing |

A cell is an execution unit, not necessarily one matrix point: the `probe`
cell produces all probe foregrounds under both backgrounds in one run
because they share the same scan. A storm or upload cell produces one
foreground under one background, because the load must be isolated. The
report has one row per load profile with the two backgrounds side by side.
The cell's
phases — background, foregrounds, client count, issue policy and stop
condition — are data in `utils/bench/scenarios.py`; one executor drives
them and neither the workload nor the executor branches on profile name.

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
| upload  | `submit`, `visible` | `idle`, `scan` | one uploaded file          | p50, p95, max | p50 only |
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
  (in-process probe samples), not the per-round medians. Storm
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

- phase guard outcomes (probe, storm, warmup): valid or not, with the
  observed start/end state and counters;
- probe rounds total and valid;
- storm wall coverage (active storm wall over scan wall, the workload's
  acceptance gate) and ops done/planned;
- warmup validity;
- failed operation counts.

A storm or idle-storm guard that cannot prove the claimed background fails
the cell before a report exists, as does a scan storm whose active wall
falls below the coverage floor. A probe round whose guard fails contributes
no samples; the cell records the exclusion count (`invalid`) and its
reasons. A probe cell that ends under its round budget fails instead of
recording a thin sample. Validity is carried into the final outputs, so a
reader can tell a stable number from a lucky one.

Warmup covers the single-client probe accesses: on the first scan of a
repetition, and before the at-rest rounds. Sustained concurrency is not
warmed: a storm's ramp is part of the load it claims to measure, and
discarding a prefix of it would be an arbitrary threshold.

## Scenarios

A profile is an execution unit; the matrix point it fills is stated below.
The profile set is `scenarios.profiles()`, and each profile's phases are
built once in `scenarios.build_spec()`.

| profile                     | fills                                                                                                                                |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `probe`                     | all probe foregrounds under `idle` and `scan`: rounds during each scan up to the round budget, then rounds at rest                   |
| `scan-storm-<kind>-<level>` | storm foreground under `scan`: sustained ops at the profile's client count until terminal, with a coverage floor                     |
| `idle-storm-<kind>-<level>` | storm foreground under `idle`: one populate scan, then a sustained burst at rest for a fixed op budget                               |
| `idle-upload`               | upload foreground under `idle`: settle the index, then upload the cell's file set at rest                                            |
| `scan-upload`               | upload foreground under `scan`: start the album scan over the materialised library, then submit the cell's file set while it indexes |

Concurrent storms outlast the scan because the question is how the request
path behaves under indexing load; an idle storm has no scan to outlast, so
its budget is a fixed op count. Both record done/planned ops. The `probe`
cell's round phase is the only one with a round budget; every other cell
runs one rep.

Both upload cells use a fixed file set: it comes from `UPLOAD_REPO` when set
(the battery pins `set-upload`), otherwise from the cell repo beyond the
staged prefix, and the cell fails when the source cannot provide that many
files. Re-uploading an image the cell already staged is not allowed: it
changes the row space the cell claims to measure. Each file records whether
a scan was active at submit and at visibility; a sample is valid when submit
saw the cell's declared background.

The probe cell is the only source of probe foreground rows.

## Image counts and matrix

The image count is the size parameter: `battery run --images N` runs the
full profile matrix against `set-N`. A cell id is `<profile>-<images>`.
There is no level above the image count.

The phase spec, not a repeat count, is the control. The `probe` cell's
scan-round phase repeats scans until it has `MIN_PROBE_ROUNDS` (20) valid
rounds; `SCAN_REPEATS` and `BATTERY_REPEATS` only bound cost. Its idle-round
phase then runs once, taking `REPEATS` measured samples per access. Every
other cell runs one rep: storms measure one
sustained burst and uploads measure one file set. The probe cell fails if it
cannot reach its round budget within the cap, instead of recording a thin
sample.

Every image count runs the same profiles with the same pinned environment;
only the repo differs. Larger image counts are opt-in for cost. The seed
tool can also prepare a large-file-class archive (`--large`); it is a repo
variant, not a matrix axis. The debug path
(`bench run cell`) takes an arbitrary profile and environment; its rows are
never recorded as baselines.

Client counts are resolved when the battery starts: `16n` becomes
`16 × NCPU` of the recording host and stays part of the profile name.

## Pipeline

```
prepare   archive → sandbox
launch    server + pinned env + identity
stimulate workload phases emit raw samples and guard events
validate  phase guards decide which samples count
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
metadata. One table per image count unless stated otherwise:

- `Measurement validity`: one row per load profile with the `idle` and
  `scan` accesses, invalid and failed counts side by side.
- `Foreground latency`: one row per load profile with the `idle` and `scan`
  statistics side by side.
- `Scan time`: a load-profile by image-count matrix showing the
  populate-scan wall clock and ingest rate; the `no-load` row is the median
  populate scan of the idle-storm cells.
- `Memory and storage footprint`: one profile-by-image-count table per
  metric, in MB, sorted by peak VMM.

Report profile labels are derived from the axis tables, never written by
hand: `probe-<access>` for the single-client probe (`probe-read`,
`probe-edit`, `probe-thumb`, `probe-scroll-bar`, `probe-scroll-rows`,
`probe-scroll-data`), `storm-<kind>-<level>` for synthetic load, and
`upload-<phase>` for ingest. `Scan time` uses `no-load`, the same storm
labels, and `upload-loop`. An execution cell keeps its own profile name
(`probe`, `scan-storm-*`, `idle-storm-*`, `idle-upload`, `scan-upload`);
the label describes the measurement, the cell describes the run. The
interval appears inline after `p50`. Stage metrics stay in the report and
baselines; the rendered summary does not repeat them.

## Prohibited patterns

The protocol rejects:

- a per-profile, per-metric or per-family branch in aggregation or
  execution;
- a metric that is measured but not accepted, or rendered but not
  compared;
- two execution paths for one label (an extra alias alongside the canonical
  profile name, a profile-name branch outside `scenarios`);
- statistics computed from summaries (median of round medians, worst of
  run worsts) except where the source only provides aggregates, as stated
  for stage metrics;
- hand-maintained metric allow-lists;
- unrecorded identity, or comparison across different identities;
- prose describing a gate, artifact or mechanism the code does not have.

## Open items

- Cross-host comparison: `16n` and tmpfs staging are host properties.
  Rows record CPU and client counts; comparing across hosts is not
  supported yet.
- Large image counts stay opt-in until the small-count baselines are stable.
- Intervals use a bootstrap over pooled raw samples. Whether a per-request
  hierarchical pooling adds information beyond that is open; the raw-sample
  interval is the current definition.
