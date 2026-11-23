# Benchmark harness

Benchmark and debug cells for the Picasu backend. The harness is stdlib-only
Python; it is not part of the server binary.

| Path                  | Role                                                                               |
| --------------------- | ---------------------------------------------------------------------------------- |
| `backend_launcher.py` | State setup, port preflight, spawn, readiness, supervision, `launcher-info.json`   |
| `workload.py`         | Image-set materialisation, scans, probes and bench transcript artifacts            |
| `run_cell.py`         | One cell: launcher, workload, failure diagnostics, graceful shutdown               |
| `battery.py`          | Pinned cell batteries, long `report.tsv`, JSON extract and baseline recording      |
| `metrics.py`          | Families, statistics, percentiles, bootstrap intervals, long row schema            |
| `compare_reports.py`  | Review a long report against accepted baselines or another report (`compare`)      |
| `baselines.tsv`       | Checked-in accepted values in long format, with intervals (updated with rationale) |
| `render_baselines.py` | Render `baselines.md` from the baselines and cell metadata only                    |
| `extract_bench.py`    | Parse harness output directories into raw samples, identity and validity           |
| `prepare.py`          | Materialise a sandbox from a canonical archive, write its manifest                 |
| `preflight.py`        | Pre-spawn checklist: binary, hyperfine, archive, sandbox manifest, port, OUT       |
| `config.py`           | Typed, validated per-layer configuration shared by every module                    |
| `manifests/`          | PhotoSpec manifests for fixed image sets                                           |

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
battery knobs.

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
python3 utils/bench/battery.py preflight --archive ~/picasu-bench/set-100 \
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
$ PORT=5795 python3 utils/bench/battery.py preflight
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
just bench run ...        # bench.py run cell ... (debug cell)
just bench battery ...    # run the default batteries into DIR (default: fresh timestamped dir)
just bench extract ...    # record baselines + render the doc from DIR (default: newest battery output)
just bench seed-images ...# bench.py seed ... (seed_images.py)   [recipe: seed-images]
just bench server ...     # bench.py server ... (backend_launcher.py)
just bench prepare ...    # bench.py prepare ... (prepare.py)
just bench status ...     # bench.py status ... (per-cell state)
just bench compare BASE NEW  # bench.py compare BASE NEW (review against baselines.tsv or a report)
bench extract ...         # bench.py extract ... (extract_bench.py, single cell)
```

`just bench run` injects the `cell` subcommand, so the recipe is invoked as
`just bench run --out ...` (no second `cell` token). `just bench battery`
runs the default image counts (50, 200, 100) into one folder; `just bench
extract` records that folder into the checked-in baselines and re-renders
the doc. Single image counts and explicit JSON dumps go through `bench.py`
directly (see below). `bench compare` reviews a report against
`baselines.tsv` (long format) or another report; it is a review tool, not
a gate.

Examples:

```sh
just bench status /tmp/picasu-battery-100
just bench battery /tmp/my-bench-run
just bench extract /tmp/my-bench-run
just bench compare utils/bench/baselines.tsv /tmp/my-bench-run/100/report.tsv
```

Direct (flag-form) invocations, for options the recipes do not expose:

```sh
python backend_launcher.py                       # env-driven server
python battery.py extract /tmp/picasu-battery-100 --out run.json
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
stage per scan: scan index, stage, n, p50_us, p95_us, max_us, context).
Each scan's stage aggregate is one sample of the `stage` family, so a stage
the backend logged with no samples contributes nothing rather than a zero.
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
extractor's functions over `server.log`, so the stage samples in
`report.tsv` and `battery extract`'s per-rep data come from one parse. A
separate `extract_bench.py` run is not required, and building a row writes
nothing into the cell outdir.

`extract_bench.py --profile <workload> <out>` labels a cell's during-scan
probes as the `scan` background for every profile, including `upload`.
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

`just bench run cell` runs one debug cell; everything
is env-driven (`OUT`, `PORT`, `BUILD_PROFILE`, `NCPU`, `IMAGES`,
`IMAGE_REPO`, `WORKLOAD`, ... — see `battery.py`, `_cell()` for the pinned
defaults: `SCAN_REPEATS=2`, hyperfine `REPEATS=3`, `WARMUP=1`). An exported
`IMAGE_HOME_SRC` selects a prepared sandbox (refused without a sandbox
manifest by the launcher preflight); `WORKLOAD_TIMEOUT_S` bounds the whole
cell (default 7200 s, sized for a single 5650-image cell — the
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

A battery is an ordered, pinned matrix of cells for one image count. The
image count is the only size parameter: `battery run --images N` runs the
full scenario matrix against `set-N`, and cells run sequentially because they
share the tempdir and the host.

| Profiles at every image count                                                     |
| --------------------------------------------------------------------------------- |
| `control`, `storm-{read,thumb,edit}-{8,16n}`, `storm-idle-read-{8,16n}`, `upload` |

A cell id is `<profile>-<images>`, e.g. `control-100` or
`storm-read-16n-100`. The storm client count (`8` or `16 × NCPU`) is part of
the profile name and is resolved once when the battery starts.

`bench run` (the `cell` command) still runs one debug cell from the
environment; debug cells are reported but never recorded as baselines.

### Measurement budget

Scan count is not a setting; it follows the budget. A cell adds scans until
it has at least `MIN_PROBE_ROUNDS` (default 20) valid probe rounds, and the
cap (`BATTERY_REPEATS`: 5 at 200 images or fewer, 3 above) only bounds cost.
Large image counts usually reach the budget in one scan. Cells without a
probe phase (storm profiles) measure every operation of one run, so they run
one scan once. A cell that cannot reach the budget fails instead of recording a thin
sample.

`battery run` resumes by default: a rep whose `status-final.json` parses is
reused (stdout says `reused`), a missing or unparseable one runs fresh. Pass
`--fresh` to redo every rep. A failing cell writes its rows when the
artifacts still parse, then stops the battery with that cell's exit code.

### Measurement model

Every row is a point in one space: `(cell, family, background, subject,
stat) -> value`; the storm client count is part of the profile name. Families are `latency` (one request),
`wall` (one scan), `stage` (one scan's observer aggregate), `upload` (one
file), `gauge` (one rep's footprint) and `count` (one timed run). Statistics
are `p50`, `p95` and `max` for every family except `count`, which is a sum.
Only `p50` carries an interval.

`docs/benchmarks.md` is the contract: axes, families, pooling rules and
prohibited patterns. This section only describes the file shapes.

### `report.tsv`

Long-format staging record, one row per `(cell, metric)`:

```
cell family background subject stat value unit n_samples n_clusters ci_low ci_high
```

Each cell is preceded by `# cell: {"identity": {...}, "validity": {...}}`.
Identity records cell, source (`battery` or `debug`), profile, images, upload
files, build, commit and scan runs; validity records probe rounds total/valid,
probe coverage, storm coverage, ops done/planned, warmup, phase-invalid count
and reasons, failed ops and reps. Values and bounds use one decimal; `count`
values are integers. `n_samples`/`n_clusters` stay in the TSV as evidence and
are not rendered.

### `baselines.tsv`

The accepted record: the same columns plus `recorded` and `note`, with a
`# cell:` identity/validity line before each cell's rows. Every metric of an
battery run is recorded; there is no allow-list. Recording
merges by `cell + family + background + subject + stat`: the
value, interval, sample counts and provenance move to the new run, the
`note` survives, and cells absent from the run carry over unchanged. A new
metric gets a `new metric recorded <date>` note.

Host environment (`environment.json`: host, kernel, CPU, governor, memory,
load, git identity) is merged as one `# env:` line per run.

### `baselines.md`

Rendered from `baselines.tsv` only (rows plus the `# cell:` metadata). Each
section has a short intro and one table per image count:

`Measurement validity`, `Idle access latency`, `During-scan access latency`,
`Concurrent load`, `Scan wall and stages`, `Upload`,
`Memory and storage footprint`, `Failed operations`, `Terms`.

Rows are labelled by profile; the image count is the sub-table heading. The
p50 interval is shown as a half-width (`7.2 ±0.3`), always present, `±n/a`
when there are fewer than two samples. `p95` and `max` are separate columns,
and raw sample counts are not rendered. The memory table is one row per
configuration, sorted by peak memory, to expose scenario-dependent spikes.

### Recording and reviewing

```sh
just bench battery                          # 50, 200, 100 into a fresh dir
just bench extract                          # record + render from the newest run
just bench extract /tmp/my-bench-run        # or an explicit run root
just bench compare utils/bench/baselines.tsv /tmp/my-bench-run/100/report.tsv
```

`compare` accepts long `report.tsv` or `baselines.tsv` on either side,
matches on the full metric key, and reports value, delta, percent change and
a flag against a tolerance (`--tolerance`, default relative band plus an
absolute floor). It refuses rows whose recorded identity (images, profile)
differs, and exits nonzero only with `--strict`; it is a review
tool, not a gate.

### `battery extract` JSON

```sh
python3 utils/bench/battery.py extract /tmp/my-battery-100 --out /tmp/my-battery-100.json
```

The document carries `battery`, `cells` (identity, validity, rows), `errors`
and `environment`. It is rebuilt from `report.tsv`, or from the rep
artifacts when no report exists. A plain extract writes no other file.
Exit codes: `0` when at least one cell parsed, `1` when the parent holds no
rep subdirs, `2` when every rep failed to parse.

### Validity knobs

- `SCAN_WARMUP` (default 1): issue one prefetch/first-image/scroll sequence
  inside the first measured scan of a probe profile and discard its timings.
- `MIN_PROBE_ROUNDS` (default 20): cell-level budget for valid probe rounds.
- `MIN_VALID_ROUNDS` (default 3): per-rep floor for probe rounds that
  started and ended while the scan was running.
- `PROBE_REFRESH_S` (default 0): rebuild the probe row list at most this
  often; 0 keeps the first non-empty list for the whole cell.
- `STORM_MIN_COVERAGE_PCT` (default 50): concurrent storm wall as a
  percentage of scan wall; below it the cell fails.
- `SCAN_TIMEOUT_S` (default 0): poll budget; 0 waits for a terminal state.
- `SCAN_STALL_TIMEOUT_S` (default 120): fail when counters stop moving.

## Workload profiles (`WORKLOAD`)

`control` (default) runs the during-scan probes and scroll probes, then the
idle probes, scroll probe and sweep after the scan settles.

Storm profiles name their access and client count explicitly:

```sh
WORKLOAD=storm-edit-8 just bench run cell --out /tmp/c-r1
WORKLOAD=storm-read-16n just bench run cell --out /tmp/c-r2
WORKLOAD=storm-idle-read-8 just bench run cell --out /tmp/c-r3
WORKLOAD=storm-idle-read-16n just bench run cell --out /tmp/c-r4
```

Concurrent storms (`storm-{read,thumb,edit}-{8,16n}`) issue ops until the
scan terminal state, with `min(STORM_CLIENTS × STORM_OPS, 256)` as a
minimum; `storm-read` is a prefetch POST, `storm-thumb` a
compressed-thumbnail GET, `storm-edit` an edit-rating PUT on a distinct
asset. Per-op latencies land in `rounds-run-N/storm-round-N.tsv`; the
reported statistics pool those ops. A concurrent storm below
`STORM_MIN_COVERAGE_PCT` or below the minimum op count fails the cell.

Idle storms (`storm-idle-<access>-<clients>`) run the populate scan, then one
fixed burst against the tree at rest, with no coverage check because there
is no concurrent scan. Their cells record done/planned ops.

`-8` is 8 clients; `-16n` is `16 × NCPU`, resolved when the battery starts
and recorded per cell (128 at NCPU=8). No unnumbered or `storm-diff` aliases
exist.

`upload` measures the write path end to end: it waits for the library index
to settle (`INDEX_SETTLE_TIMEOUT_S`, default 600 s), uploads `UPLOAD_FILES`
(default 20) files through `/post/upload`, records per-file `submit_ms` and
`visible_ms`, then runs the control probes during the next scan. Uploaded
files come from `UPLOAD_REPO` when it is set (battery cells pin `set-200`);
otherwise the cell's own repo must provide `UPLOAD_FILES` images beyond the
staged prefix, so an indexed image is never re-ingested. A source without
enough files fails the cell. Each POST is bounded by `UPLOAD_POST_TIMEOUT_S` (default 60 s) and
each file's visibility by `UPLOAD_TIMEOUT_S` (default 30 s); a file that
never becomes visible fails the cell.

Snapshot lookups page instead of reading one window: the probe list,
upload-visibility polls and the storm canary check walk the row space in
`PROBE_PAGE`-sized windows (default 1000 rows), so cells at 5650+ rows probe,
verify and see uploads beyond index 1000.

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
