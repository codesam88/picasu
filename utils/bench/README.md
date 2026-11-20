# Benchmark harness

Benchmark and debug cells for the Picasu backend. The harness is stdlib-only
Python; it is not part of the server binary.

| Path                  | Role                                                                                     |
| --------------------- | ---------------------------------------------------------------------------------------- |
| `backend_launcher.py` | State setup, port preflight, spawn, readiness, supervision, `launcher-info.json`         |
| `workload.py`         | Image-set materialisation, scans, probes and bench transcript artifacts                  |
| `run_cell.py`         | One cell: launcher, workload, failure diagnostics, graceful shutdown                     |
| `battery.py`          | Pinned cell batteries, TSV run report, baseline band check, JSON + report extract        |
| `baselines.tsv`       | Checked-in accepted bands for the gate battery (evolving record, updated with rationale) |
| `extract_bench.py`    | Summarise one or more harness output directories                                         |
| `prepare.py`          | Materialise a sandbox from a canonical archive, write its manifest                       |
| `preflight.py`        | Pre-spawn checklist: binary, hyperfine, archive, sandbox manifest, port, OUT             |
| `config.py`           | Typed, validated per-layer configuration shared by every module                          |
| `manifests/`          | PhotoSpec manifests for fixed image sets                                                 |

The environment contracts live in the module docstrings. Each producer writes
into `$OUT`; the launcher writes `launcher-info.json`, which the workload
consumes.

## Configuration

Configuration is validated once per layer, in `config.py`. Each object is
built from its inputs through a single constructor and then read as
attributes:

| Object            | Constructor | Layer                                 |
| ----------------- | ----------- | ------------------------------------- |
| `LauncherConfig`  | `from_env`  | `backend_launcher.py`: server half    |
| `WorkloadConfig`  | `from_env`  | `workload.py`: measurement half       |
| `CellConfig`      | `from_env`  | `run_cell.py`: launcher + workload    |
| `BatteryConfig`   | `from_env`  | `battery.py`: repeats, budgets, knobs |
| `PrepareConfig`   | `from_args` | `prepare.py`: sandbox preparation     |
| `PreflightConfig` | `from_env`  | `preflight.py`: checklist inputs      |

Defaults and validation rules live in `config.py`, not in the layer modules.
Shared rules — integer and float bounds, the port range, and the OUT, sandbox
and binary path checks — are single functions imported by every consumer, so
one bad variable is rejected with one message wherever it is read. All
failures raise `ConfigError`. `battery.py` builds one `BatteryConfig` per run
and derives every rep's cell environment from it; a rep never re-reads the
battery-level knobs.

Two seams stay with their layer: the workload resolves `hyperfine` through the
preflight checklist, and the launcher and orchestrator bind their own
repository root and script paths.

## Prerequisites

`hyperfine` is required for every bench cell. Install the managed build with
`just install hyperfine`; cells fail fast with an install hint if it is missing.

## Preflight checklist

`bench preflight` answers "can this cell start at all?" before any cell
process is spawned or any state is wiped. Each precondition is one line of
the form `ok <name>: <detail>` or `FAIL <name>: <detail> (hint)`; `ok`
lines go to stdout, `FAIL` lines to stderr, and the exit status is `0` only
when every check passes. A failure means the cell is refused, not run and
failed later.

```sh
just bench preflight --archive ~/picasu-bench/set-100 \
    --sandbox /dev/shm/scratchpad/sb-100 \
    --out /dev/shm/scratchpad/cell --port 5795
```

```
ok binary: /home/me/picasu/target/release/picasu
ok hyperfine: /home/me/.local/bin/hyperfine
ok archive: /home/me/picasu-bench/set-100 (100 jpg)
ok sandbox: /dev/shm/scratchpad/sb-100 (count=100)
ok out: /dev/shm/scratchpad/cell will be created
ok port: 5795 free
```

```sh
$ PORT=5795 just bench preflight
ok binary: /home/me/picasu/target/release/picasu
ok hyperfine: /home/me/.local/bin/hyperfine
FAIL port: port 5795 already in use: LISTEN 0 4096 127.0.0.1:5795 ... (pick a free PORT or stop the holder)
```

Flags override the environment: `--out` (`OUT`), `--fresh 0|1` (`FRESH`),
`--port` (`PORT`), `--sandbox` (`IMAGE_HOME_SRC`), `--binary` (`PICASU_BIN`)
and `--archive` for the archive a `bench prepare` will consume. The archive
check runs only when `--archive` is given; the sandbox check only when a
prepared sandbox is in play; the OUT check only when an OUT is configured.

The same phase runs inside a cell: every launcher start checks its binary,
sandbox manifest, OUT and port before it touches OUT or spawns the server,
and the workload config runs the hyperfine check through the same
implementation. The launcher subset skips hyperfine because a server-only
run never drives it. There is one implementation of each rule, so the
standalone checklist and the in-cell guards cannot drift apart.

## Unified CLI

`bench.py` is the single entry point; module mains are delegated to unchanged.
Every subcommand below is reachable through its own `just bench` recipe;
for anything else, run `bench.py` directly with python.

Command tree (with the recipe that runs each):

```
just bench run ...        # bench.py run cell ... (battery.py cell level)
just bench battery ...    # bench.py battery ... (battery.py run/check/extract/list)
just bench extract ...    # bench.py battery extract ... (battery parent)
just bench seed-images ...# bench.py seed ... (seed_images.py)   [recipe: seed-images]
just bench server ...     # bench.py server ... (backend_launcher.py)
just bench prepare ...    # bench.py prepare ... (prepare.py)
just bench preflight ...  # bench.py preflight ... (preflight.py)
just bench status ...     # bench.py status ... (per-cell state)
bench extract ...         # bench.py extract ... (extract_bench.py, single cell)
```

`just bench run` injects the `cell` subcommand, so the recipe is invoked as
`just bench run --out ...` (no second `cell` token). `just bench extract`
operates on a battery parent directory; the direct `bench.py extract`
invocation operates on a single cell directory. `bench battery-check` exists
only as a recipe (`just bench battery-check`), not as a `bench.py`
subcommand.

Examples:

```sh
just bench status /tmp/picasu-battery-gate
just bench extract /tmp/picasu-battery-gate /tmp/picasu-battery-gate/run.json
just bench battery gate --out /tmp/picasu-battery-gate
just bench battery-check /tmp/picasu-battery-gate/report.tsv
just bench preflight --out /dev/shm/scratchpad/cell --port 5795
```

Direct (flag-form) invocations, for options the recipes do not expose:

```sh
python backend_launcher.py                       # env-driven server
python battery.py extract /tmp/picasu-battery-gate --out run.json --format both --matrix-only
python bench.py seed --repo ~/picasu-bench/set-100 --count 100 --template manifests/foo.json --jobs 4 --large
python extract_bench.py /tmp/picasu-bench-N8-100-r1
```

## Checks

```sh
just bench test
just bench lint
```

The suite is offline-only: no subprocesses, no sockets, no external
binaries. Anything that needs a live server is verified by running
batteries, not by unit tests — if a launch fails, debug that run.

Recipes live in `utils/bench/justfile`; the root justfile only
carries `mod bench`, so there are no `bench-*` root aliases.

## Test fixtures

`tests/fixtures/` holds recorded cell artifacts, not hand-written log lines.
A parser fixture is copied byte-for-byte from a real cell (or is an exact
excerpt of one), and `tests/fixtures/README.md` records its source cell, date
and excerpt reason. Synthetic inputs are allowed only for error shapes —
unknown schema version, binary garbage, a stream cut mid-line — and stay
inline in the test that needs them, marked as synthetic.

The rule comes from the S2 miss: fabricated `observe drain:` lines in a
`harness-a.log` fixture kept the extractor tests green while every real cell
wrote its stage tables to `server.log` and extracted an empty `stages.json`.
`tests/test_recorded_fixtures.py` pins each fixture to the parser output it
must produce, so replacing a recording means updating the golden, never
editing the fixture to match the parser.

## Events

The workload writes `events.jsonl` in the cell outdir: one JSON object per
line with schema version `v` (currently 1). Event types: `probe_round`, `scroll_probe`,
`storm_sample`, `storm_summary`, `upload_file`, `upload_summary`,
`scan_state`, `verify_result`. Every object carries `v`, `type`, and `t`
(wall-clock ISO timestamp). Incompatible schema changes bump `v`; readers
switch on `v`.

`extract_bench.py` reads that stream first: `read_events` returns one typed
list per event type and `parse_log` takes its measurement fields from it.
Without a usable stream the same fields come from the per-round artifacts
(hyperfine exports, probe/storm TSVs, scan walls) that the workload writes
alongside the transcript; the transcript itself carries only status lines,
integer counts and millisecond walls — no floating-point stats. Every
fallback prints one `warn: ...` line naming the reason (a version error
names the version) and never fails the extraction. A stream counts as
complete only when its per-phase counts match the transcript's status
lines (probe rounds, hyperfine rounds, scan runs, storm summaries, canary
verifications), so a cell killed mid-run cannot report a partly written
stream.

The stream carries full-precision floats. The extractor pins every
reported value to a fixed decimal precision (fetch-round means at 4dp,
storm medians at 3dp, scan walls as integer ms), so a cell with
`events.jsonl` reports the same numbers as the same cell read from its
artifacts, and recorded cells stay comparable with checked-in baselines.

Coverage: the stream carries during-scan measurements (probe rounds, storm
summaries and canary checks, scan walls). Cell identity, the post-scan
hyperfine medians (from `hyperfine.json`) and the upload p50 columns in
`report.tsv` are still read from the cell's artifacts and transcript.
The p95/max cells of the report matrix pool raw samples the stream does
not carry — the post-scan `hyperfine.json` runs, the per-round
`hyperfine-round-*.json` exports and the per-fetch `probe-round-*.tsv`
files — so those values come from the same artifacts for a cell with or
without an event stream.

## Extractor drain tables

When `PICASU_OBSERVER_STATS=1` (default in bench mode), the backend writes
an `observe drain:` table per scan into `server.log` and one
`observe counters:` line with the exact event counts that saturate a ring
(`snapshot_memory_hit`, `snapshot_disk_fallback`). `extract_bench.py`
parses these into `stages.json` in each cell outdir (one flat row per
stage per scan: scan index, stage, n, p50_us, p95_us, max_us, context) and
adds the prefetch filter/locate path and the tree-view rebuild breakdown
(`filter_snapshot_clone`, `filter_par_iter`, `compute_locate`,
`tree_refresh_total`/`build`/`sort`/`write_wait`, `prefetch_total`) as
p50/max summary columns, plus the counter and process-metric columns.
Cells without drain tables (stats off or older logs) still parse
unchanged; their `stages.json` is an empty list.

The orchestrator writes `cell-metrics.json` in each cell while the server
is still alive. `vm_hwm_mb` is the peak resident set from
`/proc/<pid>/status` in MiB (1024-based). The `*_mb` resource columns are
decimal MB (`/1e6`): `temp_db_mb`, `index_v5_mb` and `cache_db_mb` are
apparent file sizes (`st_size`) of `temp_db.redb`, `index_v5.redb` and
`cache_db.redb`; `compressed_mb` is the apparent size of every file under
`data/object/compressed` (thumbnails/compressed storage; originals are not
under that path). `vm_hwm_mb` is MiB, the rest are MB. Cells recorded
before a metric existed report `n/a`.

The battery parses the same tables in process: `build_row` runs the
extractor's functions over `server.log`, so the report's stage columns and
`battery extract`'s per-rep `stages` come from one parse. A separate
`extract_bench.py` run is not required, and building a row writes nothing
into the cell outdir.

`extract_bench.py --profile upload <out>` labels a cell's during-scan
probes as the `uscan` background; without `--profile` they land in `scan`.
The battery passes the profile of the cell it rebuilds.

## Canonical cell

```sh
cargo build --release --bin picasu
just bench seed-images ~/picasu-bench/set-100 100
OUT=/tmp/picasu-bench-N8-100-r1 IMAGE_REPO=~/picasu-bench/set-100 \
  IMAGES=100 NCPU=8 BUILD_PROFILE=release PORT=5795 \
  just bench run cell --out /tmp/picasu-bench-N8-100-r1
just bench extract /tmp/picasu-bench-N8-100-r1
```

`just bench run cell` runs one cell via the battery cell level; everything
is env-driven (`OUT`, `PORT`, `BUILD_PROFILE`, `NCPU`, `IMAGES`,
`IMAGE_REPO`, `WORKLOAD`, ... — see `battery.py`, `_cell()` for the pinned
defaults: `SCAN_REPEATS=2`, hyperfine `REPEATS=3`, `WARMUP=1`). An exported
`IMAGE_HOME_SRC` selects a prepared sandbox (refused without a sandbox
manifest by the launcher preflight); `WORKLOAD_TIMEOUT_S` bounds the whole
cell (cell-level default 7200 s, sized for a single scale cell — the
measured 5650-row control cell needs ~1335 s). Binaries and the repo must
exist; the stack fails with a build/seed hint otherwise. `just bench
server` serves a backend without driving any workload.

An exported `IMAGE_HOME_SRC` selects a prepared sandbox: the recipe
forwards it into the cell environment and runs the preflight
sandbox-manifest check first, so an unprepared sandbox stops the recipe
before any cell process starts:

```
FAIL sandbox: IMAGE_HOME_SRC has no sandbox-manifest.json: /dev/shm/scratchpad/sb (prepare it with `bench prepare`)
```

Without `IMAGE_HOME_SRC` the check is skipped and the recipe behaves
exactly as before.

## Batteries and baselines

A battery is an ordered, pinned matrix of cells for one scale. Env and
ports are fixed by `battery.py`; cells run sequentially (shared tempdir
state). Each cell runs `BATTERY_REPEATS` times (default 3, minimum 1;
an exported `BATTERY_REPEATS` wins) into outdirs `<out>-rep1..N`, and
each run writes one row to `<out>/report.tsv`. After the N runs an extra
row named `<cell>:median` is appended. Numeric metrics are pooled across
the N repeat rows and reduced to their median, except the matrix's
`p95`/`max` columns (pooled percentile/worst) and the storm failed counts
(sum); comma-separated lists
(the per-scan `scan_walls_s` and per-scan stage p50/max values) are
pooled value by value before the median, so a two-scan repeat contributes
two samples. `n/a` and empty entries contribute nothing and a column
that is `n/a` in every repeat stays `n/a`. Identity columns (`cell`,
profile, level, scale, build, commit) are copied from the first repeat.
With `BATTERY_REPEATS=1` there is a single row per cell and no median row
(backward compatible). A failing cell writes its row when the artifacts
still parse, then stops the battery with that cell's exit code.

`battery run` resumes by default: the report is rebuilt from scratch on
every invocation, and a rep whose `status-final.json` parses (terminal
server state, artifacts intact) is reused instead of re-run — stdout
notes `reused`. A rep with a missing or unparseable marker runs fresh,
so an interrupted battery (worker timeout, Ctrl-C) continues where it
stopped instead of restarting every cell, and a retry after a failure
picks up from the reps that never finished. Fail-fast is unchanged: a
failing cell still stops the battery with its exit code. To redo every
rep from scratch, pass `--fresh` on the CLI (the `just bench battery`
recipe does not forward it):

```sh
python3 utils/bench/battery.py run gate --out /tmp/my-gate-run --fresh
```

| Level   | Images  | Cells                                                                              |
| ------- | ------- | ---------------------------------------------------------------------------------- |
| `cell`  | env     | one cell, fully parameterized by env (WORKLOAD, IMAGES, NCPU, PORT, BUILD_PROFILE) |
| `micro` | 50, 200 | `control` (at each scale)                                                          |
| `gate`  | 100     | `control`, `storm-read`, `storm-thumb`, `storm-diff`, `storm-idle`, `upload`       |
| `scale` | 5650    | `control`, `storm-diff`, `upload`                                                  |

Both levels pin `SCAN_REPEATS=2`, hyperfine `REPEATS=3`/`WARMUP=1`,
`NCPU=8`, release builds, and storm `CLIENTS=8`/`OPS=10`/`MARGIN_S=0.5`.
Image repos are seeded once (see below) and resolved from
`BENCH_SETS_ROOT` (defaults to the canonical archive root
`~/picasu-bench`). Each level also pins the
workload budget `battery run` hands to run_cell as `WORKLOAD_TIMEOUT_S`
(gate 1800 s, micro 1800 s, scale 7200 s) — `battery run` invokes run_cell
in-process, so no outer `timeout` bounds a cell. An exported
`WORKLOAD_TIMEOUT_S` overrides the level default.

The `micro` battery (50 and 200 images, control profile only, observer
stats on) is a fast, CI-friendly linearity check plus observer spot-check;
`scale` stays opt-in due to its cost.

Each level also pins `BATTERY_REPEATS=3` (overridable via the exported
`BATTERY_REPEATS` env var, minimum 1).

```sh
just bench seed-images ~/picasu-bench/set-100 100
just bench seed-images ~/picasu-bench/set-5650 5650
export BENCH_SETS_ROOT=$HOME/picasu-bench    # only if the archive lives elsewhere
just bench battery gate                      # -> /tmp/picasu-battery-gate-<stamp>/report.tsv
just bench battery gate /tmp/my-gate-run     # explicit outdir
just bench battery-check /tmp/my-gate-run/report.tsv
```

`report.tsv` carries identity (`cell`, profile, level, scale, build,
git commit from `run-manifest.json`), scan walls, the full request-latency
matrix, the extractor's stage p50/max columns for the filter/locate path
and tree-view rebuild breakdown (`n/a` when the drain table is absent), the
`observe counters` totals, and the per-cell process metrics (`vm_hwm_mb`,
`temp_db_mb`, `index_v5_mb`, `cache_db_mb`, `compressed_mb`). Upload
submit/visible p50s come from the `upload ...` log lines. The `:median`
row of a multi-repeat battery also carries `*_min`/`*_max` range columns
for the headline metrics (scan wall, scan/idle prefetch p50, scroll p50s,
`temp_db_mb`, `vm_hwm_mb`).

The matrix is one column per background × access condition × statistic:

| Column                               | Source                                                                            |
| ------------------------------------ | --------------------------------------------------------------------------------- |
| `idle_{access}_{stat}_ms`            | post-scan hyperfine runs (`hyperfine.json`), server at rest                       |
| `scan_{access}_{stat}_ms`            | during-scan probe rounds of a control/storm cell                                  |
| `uscan_{access}_{stat}_ms`           | during-scan probe rounds of the `upload` cell (bucket by profile, not by parsing) |
| `{idle,scan}_storm_{kind}_{stat}_ms` | concurrent (`scan`) or tree-at-rest (`idle`) storm bursts                         |

`access` is `prefetch`, `thumb`, `edit`, `storm_read`, `storm_thumb` or
`storm_edit`; `stat` is `p50`, `p95` or `max`. `idle_storm_*` columns are
filled only by the `storm-idle` profile, whose burst kind comes from
`IDLE_PROFILE` (default read). A column whose condition the cell did not
measure is `n/a`; `uscan_storm_*` is always `n/a` because upload never
runs a storm.

`{idle,scan}_storm_{kind}_failed` counts the ops of one storm kind that
did not return 200. It is a sum: within a cell over its runs, and across
repeats on the `:median` row.

Wall and stage columns hold one comma-separated value per scan or per
run in repeat rows (`scan_walls_s`, `tree_refresh_build_p50_us`,
`filter_snapshot_clone_max_us`, ...); a drain stage the backend logged
with no samples arrives as an explicit `0`, not `n/a`. The `:median`
row pools those values, so its figures are medians over all scans of all
repeats.

Matrix `p50` columns keep that median-over-repeats pooling, matching the
recorded baselines. The tails differ by source: `idle_*` `p95`/`max`
columns pool every timed post-scan run across all repeats and take the
percentile or worst; `scan_*`/`uscan_*` tails are the percentile or
extreme of the per-round probe summaries, then pooled like every other
per-rep metric; storm tails are the worst per-run summary. A tail
therefore describes the rounds or runs the cell actually recorded.

`baselines.tsv` is long-format (`cell`, `metric`, `baseline`, `rel_tol`,
`abs_tol`, `recorded`, `note`). The band is
`max(abs_tol, abs(baseline) * rel_tol)`. A baselined metric that is
missing, `n/a`, or outside its band fails the check with the cell and
metric named; a baselined cell missing from the report also fails.
Recording is deliberate: run the battery, review the report, edit the
baseline row, and note why in the commit. `battery-check` never writes
the file, so an unexplained move keeps failing.

`battery-check` compares baselines against the `:median` rows only:
repeat rows are recorded but never checked, so a baselined metric must be
recorded under its `<cell>:median` name (e.g. `gate-control:median`).

## Battery JSON extract

The runner owns the battery parent: `battery run` creates it and refuses to
run into a directory holding entries it did not create (anything other than
`*-repN` subdirs, `report.tsv`, `report.json`, `orchestrator.log` and the
extract's `report.md`/`matrix.tsv`).
`--fresh` wipes the parent first and is the only way past that guard. Manual
loose cells are therefore impossible.

A battery keeps a single overarching parent directory: the `--out`
argument of `battery run` (the `just bench battery` recipe) is that
parent, and it holds one `<level>-<profile>-repN` subdirectory per rep
plus `report.tsv` directly inside it. New sessions continue an
interrupted run through the same parent — resume reuses the rep subdirs
that already carry a parseable `status-final.json` instead of starting a
second battery elsewhere.

`battery extract` walks every `*-repN` subdirectory of one parent
(sorted by cell, then rep number) and dumps the whole run as one JSON
document, on stdout by default or into a file with `--out`:

```sh
just bench extract /tmp/my-gate-run
just bench extract /tmp/my-gate-run /tmp/my-gate-run.json
python3 utils/bench/battery.py extract /tmp/my-gate-run --out run.json
python3 utils/bench/battery.py extract /tmp/my-gate-run --matrix-only
python3 utils/bench/battery.py extract /tmp/my-gate-run --format tsv --matrix-only
```

One extract produces three artifacts. The JSON document (stdout, or the
`--out` file) is the machine-readable record; `report.md` is the
human-readable report; `matrix.tsv` carries the same view matrices as a
spreadsheet-readable TSV. Both human files are written next to the
JSON — into `--out`'s directory, or into the battery parent when the
JSON goes to stdout — and `--format md|tsv|both` (default `both`)
selects which. `--matrix-only` prints the human view on stdout (the
Markdown report, or the TSV with `--format tsv`) instead of the JSON
document, for quick reading.

The views roll up in three steps: rep rows pool into one
`<cell>:median` row per cell (the rows `battery check` compares), each
of the three stat matrices (p50, p95, max; rows = the six accesses,
columns = the three backgrounds) takes the median across the profile
medians that measured each background × access cell, and the
contributor counts record how many profiles contributed to every cell.
A lone contributor passes through unchanged; a cell no profile
measured is `n/a`. Both human artifacts are derived, never stored: the
JSON document keeps its shape and carries the medians they recompute
from.

`report.md` sections — all generated from the extract's rows, ranked
facts only, deterministic for one input:

| Section           | Content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| header            | battery path, level, scale, build, commit, date, cell outcomes (`completed=…, failed=…, incomplete=…, errors=…`) and a `failures:` line naming every rep dir that did not complete (failed, incomplete, or unparseable `error (…)`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `p50`/`p95`/`max` | one Markdown table per stat: rows = the six accesses, columns = the three backgrounds, values rolled up as above; then the contributor-count table — the same values `matrix.tsv` carries                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| anomalies         | `worst values`: per stat the highest single median-row value with its cell and condition; `expected data missing`: every `n/a` in a column the profile was expected to fill (its probe/storm conditions and the idle baselines, the upload p50s, the observer drain stages, the storm failed counts); `contributor counts below 2`: only the shared `idle_*` probe/baseline columns (idle prefetch/thumb/edit) that every profile fills, when their rollup rests on fewer than two profile medians — a count of 1 there means profiles stopped reporting. Structurally single-contributor columns (`scan_*`, `uscan_*`, and the storm kinds) are by design and never flagged. Conformance stays available via `battery check` (`bench battery-check`), which is not part of the report. |

Document shape (one JSON object, strict — non-finite numbers become
`null`, `n/a` stays the string `"n/a"`):

| Key       | Content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `battery` | the parent directory                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `level`   | battery level taken from the parsed rows; `null` when no rep parsed                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `cells`   | one entry per rep subdir in sorted order: `dir`, `row` (the report row via `build_row`), `manifest` (`run-manifest.json` identity subset: `dataset_id`, `dataset_count`, `commit`, `build_profile`, `observer`, `mode`, `binary_sha`; `null` for a missing or unreadable manifest), `stages` (the rep's stage rows as `build_row` parsed them from `server.log`, else a parsed `stages.json` when the cell carries no drain tables, else `[]`), `terminal` (`state` from `status-final.json`, else `"incomplete"`). A rep whose artifacts no longer parse becomes `{"dir", "error"}` and the walk continues |
| `medians` | one `<cell>:median` row per cell that contributed two or more rep rows, pooled exactly like the report's median rows; the rollup inputs `report.md` and `matrix.tsv` render from                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `report`  | the parent's `report.tsv` rows verbatim (empty list when the file is absent; a report written under an older column scheme is omitted with a warning)                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `errors`  | number of error entries among `cells`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |

Exit status: `0` when at least one rep parsed (a partial battery still
emits the document), `1` when the parent holds no rep subdirs or is not a
directory, `2` when every rep failed to parse. A corrupt `report.tsv`
fails the extract with `1`, the same way `battery check` reports it.

## Workload profiles (`WORKLOAD`)

`control` (default) runs the classic during-scan probes. Storm profiles
replace the probe rounds with a concurrent burst and check it finished
with margin before the scan ended:

```sh
WORKLOAD=storm-diff STORM_CLIENTS=8 STORM_OPS=10 just bench run cell --out /tmp/c-r1
WORKLOAD=storm-read just bench run cell --out /tmp/c-r2
WORKLOAD=storm-idle just bench run cell --out /tmp/c-r3
```

`storm-diff` fires `STORM_CLIENTS × STORM_OPS` edit-rating PUTs at
distinct assets; `storm-read` fires prefetch POSTs; `storm-thumb` fires
compressed-thumbnail GETs of random assets; `storm-idle` runs one
populate scan then storms the tree at rest (no margin check). Per-op
latencies land in `rounds-run-N/storm-round-N.tsv`; the extractor prints
medians/p95/max in the detail line. If the storm does not finish
`STORM_MARGIN_S` (default 0.5) before scan end, the cell fails with the
factor by which to increase scantime.

Snapshot lookups page instead of reading one window: the probe list,
upload-visibility polls and the storm canary check walk the row space in
`PROBE_PAGE`-sized windows (default 1000 rows per request), so cells at
5650+ rows probe, verify and see uploads beyond index 1000.

`upload` measures the write path end to end: uploads `UPLOAD_FILES`
(default `IMAGES`) files through `/post/upload`, records per-file
`submit_ms` and `visible_ms`, then runs the control probe rounds during
the subsequent scan. The stimulus first waits for the library index to
settle (`INDEX_SETTLE_TIMEOUT_S`, default 600 s) so the write path is
measured on an idle server. Each upload POST is bounded by
`UPLOAD_POST_TIMEOUT_S` (default 60 s) and each file's visibility by
`UPLOAD_TIMEOUT_S` (default 30 s); a stuck file never becomes visible,
so that budget still bounds the wait per file. Fails (exit 2) if any
uploaded file never becomes visible. Uses `IMAGE_REPO` for both the base
set and the uploaded files.

## Image repos (seeded a priori)

Cells never generate images. Seed a repo once, reuse it for any cell size
(the workload copies the first `IMAGES` entries in sorted order). The
canonical archive root is `$HOME/picasu-bench/set-N` (e.g. `set-100`,
`set-5650`): outside tmpfs and read-only to cells. Cell staging (OUT,
image home, server data) belongs on tmpfs — `/dev/shm` preferred, `/tmp`
otherwise — and `FRESH=1` (the default) may wipe OUT only under a temp
root (`/tmp`, `/var/tmp`, `/tmp/opencode`, `/dev/shm`). Materialisation
uses a hardlink when source and destination share a filesystem, falling
back to a copy (and logs which path was taken per cell):

```sh
just bench seed-images ~/picasu-bench/set-100 100        # 100 × 4000x3000 jpg
just bench seed-images ~/picasu-bench/set-500 500        # same shape, 500 images
just bench seed-images ~/picasu-bench/set-large 10 --large  # ~5MB large-file class
```

Set `OVERWRITE=1` to reseed a non-empty repo.

## Prepared sandboxes

Cells never materialise images themselves. Build a sandbox once from a
canonical archive with `bench prepare`, then point the cell at it:

```sh
just bench prepare --archive ~/picasu-bench/set-100 --sandbox /dev/shm/scratchpad/sb-100 --count 100
```

`prepare` hardlinks the first `--count` entries (sorted) into the sandbox,
falling back to a copy across filesystems, and writes
`sandbox-manifest.json` (archive, count, bytes, linkage, created,
tool_commit). The sandbox must be empty or nonexistent unless `--force`.
Large-file class archives are used as-is (`--large` is a passthrough note;
no synthesis happens here).

The launcher's `IMAGE_HOME_SRC` must point at a prepared sandbox: it fails
fast if `sandbox-manifest.json` is missing. Manual materialisation
(`MATERIALISE=0` reuse, the in-workload copy/link path) is deprecated and
remains only for unprepared legacy outdirs.
