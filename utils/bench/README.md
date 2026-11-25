# Benchmark harness

Benchmark and debug cells for the Picasu backend. The harness is stdlib-only
Python; it is not part of the server binary.

| Path                  | Role                                                                                |
| --------------------- | ----------------------------------------------------------------------------------- |
| `backend_launcher.py` | State setup, port preflight, spawn, readiness, supervision, `launcher-info.json`    |
| `workload.py`         | Image-set materialisation, shared request primitives and bench transcript artifacts |
| `scenarios.py`        | The cell matrix as data: profiles, phase specs and storm levels                     |
| `executor.py`         | One executor over the phase specs: scan session, rounds, streams, file uploads      |
| `run_cell.py`         | One cell: launcher, workload, failure diagnostics, graceful shutdown                |
| `battery.py`          | Pinned cell batteries, long `report.tsv`, JSON extract and baseline recording       |
| `metrics.py`          | Families, statistics, percentiles, bootstrap intervals, long row schema             |
| `compare_reports.py`  | Review a long report against accepted baselines or another report (`compare`)       |
| `baselines.tsv`       | Checked-in accepted values in long format, with intervals (updated with rationale)  |
| `render_baselines.py` | Render `baselines.md` from the baselines and cell metadata only                     |
| `extract_bench.py`    | Parse harness output directories into raw samples, identity and validity            |
| `prepare.py`          | Materialise a sandbox from a canonical archive, write its manifest                  |
| `preflight.py`        | Pre-spawn checklist: binary, archive, sandbox manifest, port, OUT                   |
| `config.py`           | Typed, validated per-layer configuration shared by every module                     |
| `manifests/`          | PhotoSpec manifests for fixed image sets                                            |

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
battery knobs. The launcher and orchestrator bind their own repository root
and script paths.

`scenarios.py` is the only place that names profiles: `WORKLOADS` and the
battery matrix are built from `scenarios.profiles()`, and each profile maps to
a `CellSpec` of ordered phases (background, foregrounds, clients, issue
policy, stop condition). `workload.py` resolves the spec and `executor.py`
drives it; neither branches on the profile name.

## Prerequisites

## Preflight checklist

`bench preflight` answers "can this cell start at all?" before any cell
process is spawned or any state is wiped. Each precondition is one line of the
form `ok <name>: <detail>` or `FAIL <name>: <detail> (hint)`; `ok` lines go to
stdout, `FAIL` lines to stderr, and the exit status is `0` only when every
check passes. A failure means the cell is refused, not run and failed later.

```sh
python3 utils/bench/battery.py preflight --archive ~/picasu-bench/set-100 \
    --sandbox /dev/shm/scratchpad/sb-100 \
    --out /dev/shm/scratchpad/cell --port 5795
```

```
ok binary: /home/me/picasu/target/release/picasu
ok archive: /home/me/picasu-bench/set-100 (100 jpg)
ok sandbox: /dev/shm/scratchpad/sb-100 (count=100)
ok out: /dev/shm/scratchpad/cell will be created
ok port: 5795 free
```

```sh
$ PORT=5795 python3 utils/bench/battery.py preflight
ok binary: /home/me/picasu/target/release/picasu
FAIL port: port 5795 already in use: LISTEN 0 4096 127.0.0.1:5795 ... (pick a free PORT or stop the holder)
```

Flags override the environment: `--out` (`OUT`), `--fresh 0|1` (`FRESH`),
`--port` (`PORT`), `--sandbox` (`IMAGE_HOME_SRC`), `--binary` (`PICASU_BIN`)
and `--archive` for the archive a `bench prepare` will consume. The archive
check runs only when `--archive` is given; the sandbox check only when a
prepared sandbox is in play; the OUT check only when an OUT is configured.

The same phase runs inside a cell: every launcher start checks its binary,
sandbox manifest, OUT and port before it touches OUT or spawns the server, and
the workload config runs through the same implementation. There is one
implementation of each rule, so the standalone checklist and the in-cell
guards cannot drift apart.

## Unified CLI

`bench.py` is the single entry point; module mains are delegated to unchanged.
Every subcommand below is reachable through its own `just bench` recipe; for
anything else, run `bench.py` directly with python.

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
runs the gate image counts (100, 200) into one folder; `just bench
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

The suite is offline-only: no subprocesses, no sockets, no external binaries.
Anything that needs a live server is verified by running batteries, not by
unit tests — if a launch fails, debug that run.

Recipes live in `utils/bench/justfile`; the root justfile only carries
`mod bench`, so there are no `bench-*` root aliases.

## Test fixtures

`tests/fixtures/` holds recorded cell artifacts, not hand-written log lines. A
parser fixture is copied byte-for-byte from a real cell (or is an exact
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
line with schema version `v` (currently 1). Event types: `probe_round`,
`scroll_probe`, `storm_sample`, `storm_summary`, `upload_file`,
`upload_summary`, `scan_state`, `verify_result`. Every object carries `v`,
`type`, and `t` (wall-clock ISO timestamp). Incompatible schema changes bump
`v`; readers switch on `v`.

`extract_bench.py` reads that stream: `read_events` returns one typed list per
event type and `parse_log` takes every measurement from it. There is no
artifact fallback — the stream is the measurement record, and a missing or
unusable stream fails the cell instead of being silently replaced. A stream
counts as complete only when its per-phase counts match the transcript's
status lines (probe rounds, scan runs, storm summaries, canary verifications),
so a cell killed mid-run cannot report a partly written stream.

The stream carries full-precision floats. The extractor pins every reported
value to a fixed decimal precision (fetch-round means at 4dp, storm medians at
3dp, scan walls as integer ms), so recorded cells stay comparable with
checked-in baselines.

Coverage: the stream carries during-scan measurements (probe rounds, storm
samples, summaries and canary checks, scan walls) and in-process post-scan
samples; per-file upload records carry the background that was active at
submit. Cell identity still comes from the transcript and the run manifest.

## Extractor drain tables

When `PICASU_OBSERVER_STATS=1` (default in bench mode), the backend writes an
`observe drain:` table per scan into `server.log` and one `observe counters:`
line with the exact event counts that saturate a ring (`snapshot_memory_hit`,
`snapshot_disk_fallback`). `extract_bench.py` parses these tables straight
from `server.log` into the row's stage samples (one flat row per stage per
scan: scan index, stage, n, p50_us, p95_us, max_us, context). Each scan's
stage aggregate is one sample of the `stage` family, so a stage the backend
logged with no samples contributes nothing rather than a zero. Cells without
drain tables (stats off) parse unchanged and carry no stage samples.

The orchestrator writes `cell-metrics.json` in each cell while the server is
still alive. `vm_hwm_mb` is the peak resident set from `/proc/<pid>/status` in
MiB (1024-based). The `*_mb` resource columns are decimal MB (`/1e6`):
`temp_db_mb`, `index_v5_mb` and `cache_db_mb` are apparent file sizes
(`st_size`) of `temp_db.redb`, `index_v5.redb` and `cache_db.redb`;
`compressed_mb` is the apparent size of every file under
`data/object/compressed` (thumbnails/compressed storage; originals are not
under that path). `vm_hwm_mb` is MiB, the rest are MB. Cells recorded before a
metric existed report `n/a`.

The battery parses the same tables in process: `build_row` runs the
extractor's functions over `server.log`, so the stage samples in `report.tsv`
and `battery extract`'s per-rep data come from one parse. A separate
`extract_bench.py` run is not required, and building a row writes nothing into
the cell outdir.

`extract_bench.py --profile <workload> <out>` uses the profile to resolve
storm background and kind from `scenarios`, and to identify upload cells. An
upload sample takes its report background from the event: `scan` when the
submit saw a running scan, `idle` otherwise. The battery passes the profile of
the cell it rebuilds.

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
defaults: `SCAN_REPEATS=2`, `REPEATS=3`, `WARMUP=1`). An exported
`IMAGE_HOME_SRC` selects a prepared sandbox (refused without a sandbox
manifest by the launcher preflight); `WORKLOAD_TIMEOUT_S` bounds the whole
cell (default 7200 s, sized for a single 5650-image cell — the
measured 5650-row probe cell needs ~1335 s). Binaries and the repo must
exist; the stack fails with a build/seed hint otherwise. `just bench
server` serves a backend without driving any workload.

An exported `IMAGE_HOME_SRC` selects a prepared sandbox: the recipe forwards
it into the cell environment and runs the preflight sandbox-manifest check
first, so an unprepared sandbox stops the recipe before any cell process
starts:

```
FAIL sandbox: IMAGE_HOME_SRC has no sandbox-manifest.json: /dev/shm/scratchpad/sb (prepare it with `bench prepare`)
```

Without `IMAGE_HOME_SRC` the check is skipped and the recipe behaves exactly
as before.

## Batteries and baselines

A battery is an ordered, pinned matrix of cells for one image count. The image
count is the only size parameter: `battery run --images N` runs the full
scenario matrix against `set-N`, and cells run sequentially because they share
the tempdir and the host.

| Profiles at every image count                                                                                         |
| --------------------------------------------------------------------------------------------------------------------- |
| `probe`, `scan-storm-{read,thumb,edit}-{8,16n}`, `idle-storm-{read,thumb,edit}-{8,16n}`, `idle-upload`, `scan-upload` |

The matrix is foreground × background. A cell id is `<profile>-<images>`, e.g.
`probe-100` or `scan-storm-read-16n-100`. The storm client count (`8` or
`16 × NCPU`) is part of the profile name and is resolved by
`scenarios.level_clients` when the battery starts.

`bench run` (the `cell` command) still runs one debug cell from the
environment; debug cells are reported but never recorded as baselines.

### Measurement budget

Scan count follows the phase spec. Only the `probe` cell has a round budget:
its scan-round phase repeats scans until it has `MIN_PROBE_ROUNDS`
(default 20) valid rounds, capped by `SCAN_REPEATS`; then its idle-round phase
runs. Every other cell runs exactly one rep, whether it storms under a scan,
storms a populated tree at rest, or uploads under idle or scan.
`BATTERY_REPEATS` (5 at 200 images or fewer, 3 above) caps only the probe
cell's reps.

`battery run` resumes by default: a rep whose `status-final.json` parses is
reused (stdout says `reused`), a missing or unparseable one runs fresh. Pass
`--fresh` to redo every rep. A failing cell writes its rows when the artifacts
still parse, then stops the battery with that cell's exit code.

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
storm wall coverage (the workload's gate), ops done/planned, warmup,
phase-invalid count and reasons, failed ops and reps. Values and bounds use
one decimal; `count` values are integers. `n_samples`/`n_clusters` stay in the
TSV as evidence and are not rendered.

### `baselines.tsv`

The accepted record: the same columns plus `recorded` and `note`, with a
`# cell:` identity/validity line before each cell's rows. Every metric of an
battery run is recorded; there is no allow-list. Recording merges by
`cell + family + background + subject + stat`: the value, interval, sample
counts and provenance move to the new run, the `note` survives, and cells
absent from the run carry over unchanged. A new metric gets a
`new metric recorded <date>` note.

Host environment (`environment.json`: host, kernel, CPU, governor, memory,
load, git identity) is merged as one `# env:` line per run.

### `baselines.md`

Rendered from `baselines.tsv` only (rows plus the `# cell:` metadata). Each
section has a short intro and one table per image count:

`Terms`, `Measurement validity`, `Foreground latency`, `Scan time`,
`Memory and storage footprint`.

`Foreground latency` has one table per image count with one row per load
profile. Labels are generated from the axis tables: `probe-<access>`,
`storm-<kind>-<level>`, `upload-<phase>`; the scan-time rows are `no-load`,
the storm labels and `upload-loop`. The `idle` and `scan` columns sit side by
side, so the same request shows its baseline and its loaded measurement next
to each other. `Measurement validity` also uses one table per image count and
shows the at-rest and scanning measurements side by side: accesses (the pooled
sample count behind the percentiles), `invalid` and `failed`; failed
operations are not repeated elsewhere. Memory and storage is one table per
metric (peak VMM, compressed, index DB, temp, cache), each with profile rows
and image-count columns, all in MB and sorted by peak VMM. `Scan time` is a
matrix of load profile against image count, each cell showing the
populate-scan wall clock and the resulting `images/s` rate; the `no-load` row
is the median populate scan of the idle-storm cells, and the upload row is the
serial upload loop, not a burst. Each cell contributes one scan wall, so small
differences between rows are noise. Observer stage rows stay in
`baselines.tsv` and the report; they are not part of the rendered summary.

The environment section is printed once, from the earliest recorded run, after
checking that every run agrees on host, CPU model and count, memory and git
identity. Clock frequency and load are expected to differ and are not
compared; a mismatch refuses the document.

Rows are labelled by profile; the image count is the sub-table heading. The
p50 interval is shown as a half-width (`7.2 ±0.3`), always present, `±n/a`
when there are fewer than two samples. `p95` and `max` are separate columns,
and raw sample counts are not rendered. The memory table is one row per
configuration in one fixed profile order, so a diff between recordings moves
values, not rows.

### Recording and reviewing

```sh
just bench battery                          # 50, 200, 100 into a fresh dir
just bench extract                          # record + render from the newest run
just bench extract /tmp/my-bench-run        # or an explicit run root
just bench compare utils/bench/baselines.tsv /tmp/my-bench-run/100/report.tsv
```

`compare` accepts long `report.tsv` or `baselines.tsv` on either side, matches
on the full metric key, and reports value, delta, percent change and a flag
against a tolerance (`--tolerance`, default relative band plus an absolute
floor). It refuses rows whose recorded identity (images, profile) differs, and
exits nonzero only with `--strict`; it is a review tool, not a gate.

### `battery extract` JSON

```sh
python3 utils/bench/battery.py extract /tmp/my-battery-100 --out /tmp/my-battery-100.json
```

The document carries `battery`, `cells` (identity, validity, rows), `errors`
and `environment`. It is rebuilt from `report.tsv`, or from the rep artifacts
when no report exists. A plain extract writes no other file. Exit codes: `0`
when at least one cell parsed, `1` when the parent holds no rep subdirs, `2`
when every rep failed to parse.

### Validity knobs

- `SCAN_WARMUP` (default 1): issue one prefetch/first-image/scroll sequence
  inside the first measured scan of a probe profile and discard its timings.
- `MIN_PROBE_ROUNDS` (default 20): cell-level budget for valid probe rounds.
- `MIN_VALID_ROUNDS` (default 3): per-rep floor for probe rounds that started
  and ended while the scan was running.
- `PROBE_REFRESH_S` (default 0): rebuild the probe row list at most this
  often; 0 keeps the first non-empty list for the whole cell.
- `STORM_MIN_COVERAGE_PCT` (default 50): concurrent storm wall as a percentage
  of scan wall; below it the cell fails.
- `SCAN_TIMEOUT_S` (default 0): poll budget; 0 waits for a terminal state.
- `SCAN_STALL_TIMEOUT_S` (default 120): fail when counters stop moving.

## Workload profiles (`WORKLOAD`)

`probe` (the default) runs one scan-round phase — a bundle of prefetch, edit,
thumbnail and scroll timed while the index runs — repeating scans to
`MIN_PROBE_ROUNDS`, then an idle-round phase of the same bundle at rest. Round
validity is the fraction of rounds whose guard saw the intended background;
warmup timings are discarded.

Storm profiles name their background, kind and client level:

```sh
WORKLOAD=scan-storm-edit-8 just bench run cell --out /tmp/c-r1
WORKLOAD=scan-storm-read-16n just bench run cell --out /tmp/c-r2
WORKLOAD=idle-storm-read-8 just bench run cell --out /tmp/c-r3
WORKLOAD=idle-storm-thumb-16n just bench run cell --out /tmp/c-r4
```

`scan-storm-*` runs N clients on one access until the scan terminal state,
with `min(STORM_CLIENTS × STORM_OPS, 256)` as a minimum; `read` is a prefetch
POST, `thumb` a compressed-thumbnail GET, `edit` an edit-rating PUT on a
distinct asset. The reported statistics pool the per-op `storm_sample` events.
The cell fails if the storm covers less than `STORM_MIN_COVERAGE_PCT` of the
scan wall or falls short of the minimum op count. Coverage is computed from
the scan session's started and ended instants after the session finishes.

`idle-storm-*` first runs a populate scan, then one fixed burst of
`STORM_CLIENTS × STORM_OPS` ops against the tree at rest, guarded by an idle
phase guard. There is no coverage check because there is no concurrent scan;
the cells record done/planned ops.

`-8` is 8 clients; `-16n` is `16 × NCPU`, resolved by
`scenarios.level_clients` when the battery starts and recorded per cell (128
at NCPU=8). The profile set in `scenarios.profiles()` is exhaustive: no
unnumbered level, alternate kind name or legacy alias exists.

`idle-upload` and `scan-upload` measure the write path. Both wait for the
library index to settle (`INDEX_SETTLE_TIMEOUT_S`, default 600 s) before the
upload phase, then upload `UPLOAD_FILES` (default 20) files serially through
`/post/upload`, recording per-file `submit_ms` and `visible_ms` and whether a
scan was active at submit and at visibility. `scan-upload` runs the upload
phase under a scan; `idle-upload` runs it at rest. An upload sample's report
background is `scan` for `scan-upload` and `idle` for `idle-upload`, and
Uploaded files come from `UPLOAD_REPO` when it is set (battery cells pin
`set-upload` for both profiles); otherwise the cell's own repo must provide
`UPLOAD_FILES` images beyond the staged prefix, so an indexed image is never
re-ingested. A source without enough files fails the cell. Each POST is
bounded by `UPLOAD_POST_TIMEOUT_S` (default 60 s) and each file's visibility
by `UPLOAD_TIMEOUT_S` (default 120 s); a file that never becomes visible fails
the cell.

Snapshot lookups page instead of reading one window: the probe list,
upload-visibility polls and the storm canary check walk the row space in
`PROBE_PAGE`-sized windows (default 1000 rows), so cells at 5650+ rows probe,
verify and see uploads beyond index 1000.

## Image repos (seeded a priori)

Cells never generate images. Seed a repo once, reuse it for any cell size (the
workload copies the first `IMAGES` entries in sorted order). The canonical
archive root is `$HOME/picasu-bench/set-N` (e.g. `set-100`, `set-5650`):
outside tmpfs and read-only to cells. Cell staging (OUT, image home, server
data) belongs on tmpfs — `/dev/shm` preferred, `/tmp` otherwise — and
`FRESH=1` (the default) may wipe OUT only under a temp root (`/tmp`,
`/var/tmp`, `/tmp/opencode`, `/dev/shm`). Materialisation uses a hardlink when
source and destination share a filesystem, falling back to a copy (and logs
which path was taken per cell):

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
falling back to a copy across filesystems, and writes `sandbox-manifest.json`
(archive, count, bytes, linkage, created, tool_commit). The sandbox must be
empty or nonexistent unless `--force`. Large-file class archives are used
as-is (`--large` is a passthrough note; no synthesis happens here).

The launcher's `IMAGE_HOME_SRC` must point at a prepared sandbox: it fails
fast if `sandbox-manifest.json` is missing. Manual materialisation
(`MATERIALISE=0` reuse, the in-workload copy/link path) is deprecated and
remains only for unprepared legacy outdirs.
