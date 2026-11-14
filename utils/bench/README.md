# Benchmark harness

Benchmark and debug cells for the Picasu backend. The harness is stdlib-only
Python plus the shell workload driver; it is not part of the server binary.

| Path                  | Role                                                                             |
| --------------------- | -------------------------------------------------------------------------------- |
| `backend_launcher.py` | State setup, port preflight, spawn, readiness, supervision, `launcher-info.json` |
| `launch-backend.py`   | Env-driven CLI wrapper around `backend_launcher.py`                              |
| `workload.py`         | Image-set materialisation, scans, probes and harness-A-compatible artifacts      |
| `run-workload.py`     | Env-driven CLI wrapper around `workload.py`                                      |
| `run-cell.py`         | One cell: launcher, workload, failure diagnostics, graceful shutdown             |
| `battery.py`          | Pinned cell batteries, TSV run report, baseline band check                       |
| `baselines.tsv`       | Checked-in accepted bands for the gate battery                                   |
| `extract-bench.py`    | Summarise one or more harness output directories                                 |
| `manifests/`          | PhotoSpec manifests for fixed image sets                                         |

The environment contracts live in the module docstrings. Each producer writes
into `$OUT`; the launcher writes `launcher-info.json`, which the workload
consumes.

## Prerequisites

`hyperfine` is required for every bench cell. Install the managed build with
`just install hyperfine`; cells fail fast with an install hint if it is missing.

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
just bench seed-images /tmp/sets/set-100 100
just bench workload /tmp/picasu-bench-N8-100-r1 /tmp/sets/set-100 100 8
just bench extract /tmp/picasu-bench-N8-100-r1
```

`just bench workload <out> <image_repo> <images> <ncpu> [debug|release]`
runs the canonical cell (`build` and `port` take `name=value` overrides).
Binaries and the repo must exist; the stack fails with a build/seed hint
otherwise. `just bench server` serves a backend without driving any
workload.

## Batteries and baselines

A battery is an ordered, pinned matrix of cells for one scale. Env and
ports are fixed by `battery.py`; cells run sequentially (shared tempdir
state) and each writes one row to `<out>/report.tsv`. A failing cell
writes its row when the artifacts still parse, then stops the battery
with that cell's exit code.

| Level   | Images | Cells                                                                        |
| ------- | ------ | ---------------------------------------------------------------------------- |
| `gate`  | 100    | `control`, `storm-read`, `storm-thumb`, `storm-diff`, `storm-idle`, `upload` |
| `scale` | 5650   | `control`, `storm-diff`, `upload`                                            |

Both levels pin `SCAN_REPEATS=2`, hyperfine `REPEATS=3`/`WARMUP=1`,
`NCPU=8`, release builds, and storm `CLIENTS=8`/`OPS=10`/`MARGIN_S=0.5`.
Image repos are seeded once (see below) and resolved from
`BENCH_SETS_ROOT` (default `/tmp/sets`).

```sh
just bench seed-images /tmp/sets/set-100 100
just bench seed-images /tmp/sets/set-5650 5650
just bench battery gate                     # -> /tmp/picasu-battery-gate-<stamp>/report.tsv
just bench battery gate /tmp/my-gate-run    # explicit outdir
just bench battery-check /tmp/my-gate-run/report.tsv
```

`report.tsv` carries identity (`cell`, profile, level, scale, build,
git commit from `run-manifest.json`), scan walls, request p50s/worsts
and idle baselines, upload submit/visible p50s (parsed from the
`upload ...` log lines), and the extractor's `tree_refresh_build` /
`filter_snapshot_clone` p50/max columns (`n/a` when the drain table is
absent). Storm aggregates remain in the cell outdir and extractor table.

`baselines.tsv` is long-format (`cell`, `metric`, `baseline`, `rel_tol`,
`abs_tol`, `recorded`, `note`). The band is
`max(abs_tol, abs(baseline) * rel_tol)`. A baselined metric that is
missing, `n/a`, or outside its band fails the check with the cell and
metric named; a baselined cell missing from the report also fails.
Recording is deliberate: run the battery, review the report, edit the
baseline row, and note why in the commit. `battery-check` never writes
the file, so an unexplained move keeps failing.

## Workload profiles (`WORKLOAD`)

`control` (default) runs the classic during-scan probes. Storm profiles
replace the probe rounds with a concurrent burst and check it finished
with margin before the scan ended:

```sh
WORKLOAD=storm-diff STORM_CLIENTS=8 STORM_OPS=10 just bench workload /tmp/c-r1 /tmp/sets/set-100 100 8
WORKLOAD=storm-read just bench workload /tmp/c-r2 /tmp/sets/set-100 100 8
WORKLOAD=storm-idle just bench workload /tmp/c-r3 /tmp/sets/set-100 100 8
```

`storm-diff` fires `STORM_CLIENTS × STORM_OPS` edit-rating PUTs at
distinct assets; `storm-read` fires prefetch POSTs; `storm-thumb` fires
compressed-thumbnail GETs of random assets; `storm-idle` runs one
populate scan then storms the tree at rest (no margin check). Per-op
latencies land in `rounds-run-N/storm-round-N.tsv`; the extractor prints
medians/p95/max in the detail line. If the storm does not finish
`STORM_MARGIN_S` (default 5) before scan end, the cell fails with the
factor by which to increase scantime.

`upload` measures the write path end to end: uploads `UPLOAD_FILES`
(default `IMAGES`) files through `/post/upload`, records per-file
`submit_ms` and `visible_ms`, then runs the control probe rounds during
the subsequent scan. Fails (exit 2) if any uploaded file never becomes
visible. Uses `IMAGE_REPO` for both the base set and the uploaded files.

## Image repos (seeded a priori)

Cells never generate images. Seed a repo once, reuse it for any cell size
(the workload copies the first `IMAGES` entries in sorted order). Materialisation
uses a hardlink when source and destination share a filesystem, falling back to
a copy (and logs which path was taken per cell):

```sh
just bench seed-images /tmp/sets/set-100 100        # 100 × 4000x3000 jpg
just bench seed-images /tmp/sets/set-500 500        # same shape, 500 images
just bench seed-images /tmp/sets/set-large 10 --large  # ~5MB large-file class
```

Set `OVERWRITE=1` to reseed a non-empty repo. A prepared root can be reused
without copying by setting `MATERIALISE=0` (the workload reuses `IMAGE_PATH`
as-is and logs the dataset identity). The launcher's `IMAGE_HOME_SRC` points
`PICASU_IMAGE_HOME` at such a root.
