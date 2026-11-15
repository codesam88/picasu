# Benchmark harness

Benchmark and debug cells for the Picasu backend. The harness is stdlib-only
Python; it is not part of the server binary.

| Path                  | Role                                                                             |
| --------------------- | -------------------------------------------------------------------------------- |
| `backend_launcher.py` | State setup, port preflight, spawn, readiness, supervision, `launcher-info.json` |
| `launch-backend.py`   | Env-driven CLI wrapper around `backend_launcher.py`                              |
| `workload.py`         | Image-set materialisation, scans, probes and bench transcript artifacts          |
| `run-workload.py`     | Env-driven CLI wrapper around `workload.py`                                      |
| `run-cell.py`         | One cell: launcher, workload, failure diagnostics, graceful shutdown             |
| `battery.py`          | Pinned cell batteries, TSV run report, baseline band check, JSON extract         |
| `baselines.tsv`       | Checked-in accepted bands for the gate battery                                   |
| `extract-bench.py`    | Summarise one or more harness output directories                                 |
| `prepare.py`          | Materialise a sandbox from a canonical archive, write its manifest               |
| `preflight.py`        | Pre-spawn checklist: binary, hyperfine, archive, sandbox manifest, port, OUT     |
| `manifests/`          | PhotoSpec manifests for fixed image sets                                         |

The environment contracts live in the module docstrings. Each producer writes
into `$OUT`; the launcher writes `launcher-info.json`, which the workload
consumes.

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

## Checks

```sh
just bench test
just bench lint
```

Recipes live in `utils/bench/justfile`; the root justfile only
carries `mod bench`, so there are no `bench-*` root aliases.

## Extractor drain tables

When `PICASU_OBSERVER_STATS=1` (default in bench mode), the backend writes
an `observe drain:` table per scan into `server.log`. `extract-bench.py`
parses these into `stages.json` in each cell outdir (one flat row per
stage per scan: scan index, stage, n, p50_us, p95_us, max_us, context) and
adds `tree_refresh_build_*` / `filter_snapshot_clone_*` summary columns to
the printed table. Cells without drain tables (stats off or older logs)
still parse unchanged; their `stages.json` is an empty list.

## Canonical cell

```sh
cargo build --release --bin picasu
just bench seed-images ~/picasu-bench/set-100 100
just bench workload /tmp/picasu-bench-N8-100-r1 ~/picasu-bench/set-100 100 8
just bench extract /tmp/picasu-bench-N8-100-r1
```

`just bench workload <out> <image_repo> [images] [ncpu] [build] [port]`
runs the canonical cell. Recipe parameters are positional — just 1.59
has no `name=value` overrides — so `build` is the 5th argument
(`debug`/`release`, default `release`) and `port` the 6th (default 5795):

```sh
just bench workload /tmp/c-r1 ~/picasu-bench/set-100 100 8 release 5801
```

`TIMEOUT` bounds the whole cell (default 3600 s), sized for a single
scale cell (the measured 5650-row control cell needs ~1335 s). Binaries
and the repo must exist; the stack fails with a build/seed hint
otherwise. `just bench server` serves a backend without driving any
workload.

## Batteries and baselines

A battery is an ordered, pinned matrix of cells for one scale. Env and
ports are fixed by `battery.py`; cells run sequentially (shared tempdir
state). Each cell runs `BATTERY_REPEATS` times (default 3, minimum 1;
an exported `BATTERY_REPEATS` wins) into outdirs `<out>-rep1..N`, and
each run writes one row to `<out>/report.tsv`. After the N runs an extra
row named `<cell>:median` is appended. Numeric metrics are pooled across
the N repeat rows and reduced to their median; comma-separated lists
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

| Level   | Images | Cells                                                                        |
| ------- | ------ | ---------------------------------------------------------------------------- |
| `gate`  | 100    | `control`, `storm-read`, `storm-thumb`, `storm-diff`, `storm-idle`, `upload` |
| `scale` | 5650   | `control`, `storm-diff`, `upload`                                            |

Both levels pin `SCAN_REPEATS=2`, hyperfine `REPEATS=3`/`WARMUP=1`,
`NCPU=8`, release builds, and storm `CLIENTS=8`/`OPS=10`/`MARGIN_S=0.5`.
Image repos are seeded once (see below) and resolved from
`BENCH_SETS_ROOT` (default `/tmp/sets`; point it at the canonical
archive root when the repos live there). Each level also pins the
workload budget `battery run` hands to run-cell as `WORKLOAD_TIMEOUT_S`
(gate 1800 s, scale 7200 s) — `battery run` invokes run-cell
in-process, so no outer `timeout` bounds a cell. An exported
`WORKLOAD_TIMEOUT_S` overrides the level default.

Each level also pins `BATTERY_REPEATS=3` (overridable via the exported
`BATTERY_REPEATS` env var, minimum 1).

```sh
just bench seed-images ~/picasu-bench/set-100 100
just bench seed-images ~/picasu-bench/set-5650 5650
export BENCH_SETS_ROOT=$HOME/picasu-bench    # battery repo root (default /tmp/sets)
just bench battery gate                      # -> /tmp/picasu-battery-gate-<stamp>/report.tsv
just bench battery gate /tmp/my-gate-run     # explicit outdir
just bench battery-check /tmp/my-gate-run/report.tsv
```

`report.tsv` carries identity (`cell`, profile, level, scale, build,
git commit from `run-manifest.json`), scan walls, request p50s/worsts
and idle baselines, upload submit/visible p50s (parsed from the
`upload ...` log lines), and the extractor's `tree_refresh_build` /
`filter_snapshot_clone` p50/max columns (`n/a` when the drain table is
absent). Storm aggregates remain in the cell outdir and extractor table.

Wall and stage columns hold one comma-separated value per scan or per
run in repeat rows (`scan_walls_s`, `tree_refresh_build_p50_us`,
`filter_snapshot_clone_max_us`, ...); a drain stage the backend logged
with no samples arrives as an explicit `0`, not `n/a`. The `:median`
row pools those values, so its figures are medians over all scans of all
repeats.

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
just bench battery-extract /tmp/my-gate-run
just bench battery-extract /tmp/my-gate-run /tmp/my-gate-run.json
python3 utils/bench/battery.py extract /tmp/my-gate-run --out run.json
```

Document shape (one JSON object, strict — non-finite numbers become
`null`, `n/a` stays the string `"n/a"`):

| Key       | Content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `battery` | the parent directory                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `level`   | battery level taken from the parsed rows; `null` when no rep parsed                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `cells`   | one entry per rep subdir in sorted order: `dir`, `row` (the report row via `build_row`), `manifest` (`run-manifest.json` identity subset: `dataset_id`, `dataset_count`, `commit`, `build_profile`, `observer`, `mode`, `binary_sha`; `null` for a missing or unreadable manifest), `stages` (parsed `stages.json`, else `[]`), `terminal` (`state` from `status-final.json`, else `"incomplete"`). A rep whose artifacts no longer parse becomes `{"dir", "error"}` and the walk continues |
| `medians` | one `<cell>:median` row per cell that contributed two or more rep rows, pooled exactly like the report's median rows                                                                                                                                                                                                                                                                                                                                                                        |
| `report`  | the parent's `report.tsv` rows verbatim (empty list when the file is absent)                                                                                                                                                                                                                                                                                                                                                                                                                |
| `errors`  | number of error entries among `cells`                                                                                                                                                                                                                                                                                                                                                                                                                                                       |

Exit status: `0` when at least one rep parsed (a partial battery still
emits the document), `1` when the parent holds no rep subdirs or is not a
directory, `2` when every rep failed to parse. A corrupt `report.tsv`
fails the extract with `1`, the same way `battery check` reports it.

## Workload profiles (`WORKLOAD`)

`control` (default) runs the classic during-scan probes. Storm profiles
replace the probe rounds with a concurrent burst and check it finished
with margin before the scan ended:

```sh
WORKLOAD=storm-diff STORM_CLIENTS=8 STORM_OPS=10 just bench workload /tmp/c-r1 ~/picasu-bench/set-100 100 8
WORKLOAD=storm-read just bench workload /tmp/c-r2 ~/picasu-bench/set-100 100 8
WORKLOAD=storm-idle just bench workload /tmp/c-r3 ~/picasu-bench/set-100 100 8
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
