#!/usr/bin/env python3
"""Battery runner and baseline checker.

A battery is an ordered, pinned set of benchmark cells for one scale. Each
cell runs through `run_cell.py`; after the cell finishes the
extractor's parsed values become one row in a TSV run report. Reports are
compared against `baselines.tsv`, where every accepted metric carries one
recorded value; a reported median fails the check when it is neither
within 50% of the recorded value nor within a small absolute floor in
the metric's own unit.

Usage:
    utils/bench/battery.py run gate --out /tmp/picasu-battery-gate
    utils/bench/battery.py check /tmp/picasu-battery-gate/report.tsv
    utils/bench/battery.py list gate
    utils/bench/battery.py extract /tmp/picasu-battery-gate [--out run.json] \\
        [--matrix-only] [--baselines-tsv PATH] [--render-md PATH]

`run` resumes by default: the report is rebuilt from scratch on every
invocation and each rep whose `status-final.json` parses is reused
instead of re-run, so an interrupted battery continues where it stopped.
`--fresh` re-runs every rep.

`extract` dumps one battery parent (its `*-repN` subdirs plus
`report.tsv`) as a single JSON document: per-rep report rows, manifest
identity, stages, terminal state, pooled medians and the report rows.
The same run renders the pooled medians into the human artifacts beside
the JSON: `report.md` (header, p50/p95/max matrices, anomalies) and
`matrix.tsv` (the matrices as a spreadsheet-readable
TSV); `--format md|tsv|both` selects which, and `--matrix-only` prints
the human view on stdout instead of the JSON document. `--baselines-tsv`
merges every parent's pooled medians into one long-format baselines file;
`--render-md` additionally renders the full `baselines.md` from the merged
values plus every parent's `report.tsv`. One invocation records a whole
multi-level battery.

Environment:
    OUT                  battery directory for `run` when --out is absent
    BENCH_SETS_ROOT      image repo root (default ~/picasu-bench)
    BENCH_PORT           first cell port (default 5795); cells increment
    IMAGE_PATH           passed through to each cell by run_cell.py
    WORKLOAD_TIMEOUT_S   per-cell workload budget handed to run_cell
                         (default: gate 1800, scale 7200); an exported
                         value overrides the level default

    BATTERY_REPEATS  number of times each cell runs; the median row becomes
                     the comparison unit (default: 3, minimum 1; 1 = single
                     row per cell, no median row). An exported value wins.

Batteries pin every knob that changes a measurement (`SCAN_REPEATS`,
hyperfine `REPEATS`/`WARMUP`, storm client/op counts and margin) plus the
build profile, image set and CPU count. Cells run sequentially: backend
tests and bench cells share tempdir state, so parallel cells would
contaminate each other.

Baselines are deliberate: `check` never updates the file. A reported
median outside the guardrail fails until the move is explained and the
row is re-recorded.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import shutil
import signal
import statistics
import subprocess
import sys
from dataclasses import dataclass
from datetime import date, datetime
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))
import config  # noqa: E402
import environment  # noqa: E402
import extract_bench  # noqa: E402
import run_cell  # noqa: E402
from config import DEFAULT_SETS_ROOT, ConfigError  # noqa: E402

DEFAULT_BASE_PORT = config.BATTERY_BASE_PORT
NA = "n/a"

# One row per cell in the report. Latency columns come from the extractor's
# report matrix (`BACKGROUNDS` x `ACCESSES` x `STATS`): every background the
# battery exercises carries its own median/p95/worst pair, so no cell's row
# is comparable only through a column another profile fills. The failed
# columns count ops a storm kind could not complete, pooled across reps.
IDENTITY_COLUMNS = ["cell", "profile", "level", "scale", "build", "commit"]
STAGE_COLUMNS = list(extract_bench.STAGE_COLUMNS)
COUNTER_COLUMNS = list(extract_bench.COUNTER_COLUMNS)
METRICS_COLUMNS = list(extract_bench.METRICS_COLUMNS)
SCROLL_COLUMNS = list(extract_bench.SCROLL_COLUMNS)
UPLOAD_COLUMNS = ["upload_submit_p50_ms", "upload_visible_p50_ms"]

HEADLINE_RANGE_COLUMNS = [
    "scan_wall_s",
    "scan_prefetch_p50_ms",
    "idle_prefetch_p50_ms",
    "scroll_bar_p50_ms",
    "scroll_rows_p50_ms",
    "scroll_data_p50_ms",
    "temp_db_mb",
    "vm_hwm_mb",
]
RANGE_COLUMNS = [
    f"{column}_{suffix}" for column in HEADLINE_RANGE_COLUMNS for suffix in ("min", "max")
]

BASELINE_COLUMNS = ["cell", "metric", "baseline", "recorded", "note"]
BASELINE_HEADER = "\t".join(BASELINE_COLUMNS)

# Metrics a battery records as baselines, in file order. Only the report
# columns here become baselined rows; scroll, resource and range columns
# stay informational.
BASELINE_METRICS = (
    "scan_wall_s",
    "scan_prefetch_p50_ms",
    "scan_prefetch_max_ms",
    "scan_thumb_p50_ms",
    "scan_thumb_max_ms",
    "scan_edit_p50_ms",
    "scan_edit_max_ms",
    "uscan_prefetch_p50_ms",
    "uscan_prefetch_max_ms",
    "uscan_thumb_p50_ms",
    "uscan_thumb_max_ms",
    "uscan_edit_p50_ms",
    "uscan_edit_max_ms",
    "idle_thumb_p50_ms",
    "idle_prefetch_p50_ms",
    "idle_edit_p50_ms",
    "upload_submit_p50_ms",
    "upload_visible_p50_ms",
    "tree_refresh_build_p50_us",
    "tree_refresh_build_max_us",
    "filter_snapshot_clone_p50_us",
    "filter_snapshot_clone_max_us",
)

# Deterministic baseline order: micro scales, then gate cells.
CELL_ORDER = {
    "micro-control-50:median": 0,
    "micro-control-200:median": 1,
    "gate-control:median": 2,
    "gate-storm-read:median": 3,
    "gate-storm-thumb:median": 4,
    "gate-storm-diff:median": 5,
    "gate-storm-idle:median": 6,
    "gate-upload:median": 7,
}


_UPLOAD_P50 = re.compile(r"upload submit_p50=(\d+) visible_p50=(\d+)")


# The extractor owns the matrix column names; the report is that scheme plus
# the scan walls and the server-side stage cost columns.
LATENCY_COLUMNS = list(extract_bench.LATENCY_COLUMNS)
STORM_FAILED_COLUMNS = list(extract_bench.STORM_FAILED_COLUMNS)
REPORT_COLUMNS = (
    IDENTITY_COLUMNS
    + ["scan_wall_s", "scan_walls_s"]
    + LATENCY_COLUMNS
    + STORM_FAILED_COLUMNS
    + UPLOAD_COLUMNS
    + STAGE_COLUMNS
    + COUNTER_COLUMNS
    + METRICS_COLUMNS
    + SCROLL_COLUMNS
    + RANGE_COLUMNS
)

# Derived descriptions for the report schema. The md generator renders
# these beside the tables so a metric's unit and provenance come from the
# column name and the column lists above, not from prose maintained by hand.
_LATENCY_COLUMN_RE = re.compile(
    r"^(idle|scan|uscan)_(prefetch|thumb|edit|storm_[a-z]+)_(p50|p95|max)_ms$"
)
_STAGE_COLUMN_RE = re.compile(r"^(.+)_(p50|max)_us$")
_SCROLL_COLUMN_RE = re.compile(r"^scroll_(bar|rows|data)_(p50|p95|max)_ms$")
_STORM_FAILED_RE = re.compile(r"^(idle|scan)_storm_(read|thumb|edit)_failed$")
_BACKGROUND_SOURCE = {
    "idle": "post-scan, server at rest",
    "scan": "during-scan",
    "uscan": "upload-cell during-scan",
}
_RESOURCE_METADATA = {
    "vm_hwm_mb": ("MiB", "peak", "process resident set"),
    "temp_db_mb": ("MB", "size", "temp_db.redb apparent size"),
    "index_v5_mb": ("MB", "size", "index_v5.redb apparent size"),
    "cache_db_mb": ("MB", "size", "cache_db.redb apparent size"),
    "compressed_mb": ("MB", "size", "compressed object store apparent size"),
}


def report_column_metadata(column: str) -> dict:
    """Unit, statistic and source for one report column, derived by name."""
    if column in IDENTITY_COLUMNS:
        return {"unit": "-", "statistic": "-", "source": "cell identity"}
    if column == "scan_wall_s":
        return {"unit": "s", "statistic": "median", "source": "scan wall"}
    if column == "scan_walls_s":
        return {"unit": "s", "statistic": "list", "source": "per-scan walls"}
    if column in UPLOAD_COLUMNS:
        stage = "submit" if "submit" in column else "visible"
        return {"unit": "ms", "statistic": "p50", "source": f"upload stimulus ({stage})"}
    match = _LATENCY_COLUMN_RE.match(column)
    if match:
        background, access, stat = match.groups()
        if access.startswith("storm_"):
            access = f"storm burst ({access.removeprefix('storm_')})"
        return {
            "unit": "ms",
            "statistic": stat,
            "source": f"{_BACKGROUND_SOURCE[background]}, {access}",
        }
    match = _STORM_FAILED_RE.match(column)
    if match:
        background, kind = match.groups()
        return {
            "unit": "ops",
            "statistic": "sum",
            "source": f"{background} storm {kind}, non-200 responses",
        }
    match = _STAGE_COLUMN_RE.match(column)
    if match:
        stage, stat = match.groups()
        return {"unit": "us", "statistic": stat, "source": f"observer stage {stage}"}
    if column in COUNTER_COLUMNS:
        return {"unit": "events", "statistic": "sum", "source": "observer counter"}
    if column in METRICS_COLUMNS:
        unit, stat, source = _RESOURCE_METADATA[column]
        return {"unit": unit, "statistic": stat, "source": source}
    match = _SCROLL_COLUMN_RE.match(column)
    if match:
        what, stat = match.groups()
        return {"unit": "ms", "statistic": stat, "source": f"scroll probe ({what})"}
    if column.endswith(("_min", "_max")):
        base, stat = column.rsplit("_", 1)
        if base in HEADLINE_RANGE_COLUMNS:
            metadata = report_column_metadata(base)
            metadata["statistic"] = stat
            metadata["source"] = f"range of {base} across repeat rows"
            return metadata
    return {"unit": "-", "statistic": "-", "source": "report column"}


# --- battery definition ------------------------------------------------------
@dataclass(frozen=True)
class Cell:
    level: str
    profile: str
    images: int
    repo: str
    ncpu: int
    env: dict
    outdir_suffix: str
    name: str = ""

    @property
    def cell_id(self) -> str:
        return self.name or f"{self.level}-{self.profile}"


def _cell(
    level: str,
    profile: str,
    images: int,
    repo: str,
    *,
    cell_name: str | None = None,
    outdir_suffix: str | None = None,
) -> Cell:
    env = {
        "WORKLOAD": profile,
        "IMAGES": str(images),
        "NCPU": "8",
        "BUILD_PROFILE": "release",
        "SCAN_REPEATS": "2",
        "REPEATS": "3",
        "WARMUP": "1",
    }
    if profile.startswith("storm-"):
        env.update({"STORM_CLIENTS": "8", "STORM_OPS": "10", "STORM_MARGIN_S": "0.5"})
    if profile == "upload":
        env["UPLOAD_FILES"] = str(images)
    return Cell(
        level=level,
        profile=profile,
        images=images,
        repo=repo,
        ncpu=8,
        env=env,
        outdir_suffix=outdir_suffix if outdir_suffix is not None else f"{level}-{profile}",
        name=cell_name if cell_name is not None else "",
    )


_BATTERIES = {
    "gate": {
        "images": 100,
        "repo": "set-100",
        "profiles": ("control", "storm-read", "storm-thumb", "storm-diff", "storm-idle", "upload"),
    },
    "scale": {
        "images": 5650,
        "repo": "set-5650",
        "profiles": ("control", "storm-diff", "upload"),
    },
    "micro": {
        "scales": ((50, "set-50"), (200, "set-200")),
        "profiles": ("control",),
    },
}


_KNOWN_LEVELS = ("cell", "gate", "micro", "scale")


def _cell_from_env(environ: dict) -> Cell:
    """Build the single cell for the `cell` battery level from the environment."""
    environ = environ or {}
    profile = environ.get("WORKLOAD", "control")
    if profile not in config.WORKLOADS:
        raise config.ConfigError(f"WORKLOAD must be one of {config.WORKLOADS}, got '{profile}'")

    images = config.integer(environ, "IMAGES", config.DEFAULT_IMAGES, minimum=1)
    ncpu = config.integer(environ, "NCPU", 8, minimum=0)
    build_profile = config.validate_build_profile(environ, "release")

    # Validate the remaining env keys up front so a bad cell is refused
    # before any process spawns (reuse the shared config rules).
    if "PORT" in environ:
        config.validate_port(environ["PORT"])
    scan_repeats = config.integer(environ, "SCAN_REPEATS", 2, minimum=1)
    hyperfine_repeats = config.integer(environ, "REPEATS", config.DEFAULT_REPEATS, minimum=1)
    warmup = config.integer(environ, "WARMUP", config.DEFAULT_WARMUP, minimum=0)
    if "WARMUP" in environ:
        pass

    storm_clients = (
        config.integer(environ, "STORM_CLIENTS", config.DEFAULT_STORM_CLIENTS, minimum=1)
        if "STORM_CLIENTS" in environ
        else config.DEFAULT_STORM_CLIENTS
    )
    storm_ops = (
        config.integer(environ, "STORM_OPS", config.DEFAULT_STORM_OPS, minimum=1)
        if "STORM_OPS" in environ
        else config.DEFAULT_STORM_OPS
    )
    storm_margin = (
        config.number(environ, "STORM_MARGIN_S", config.DEFAULT_STORM_MARGIN_S)
        if "STORM_MARGIN_S" in environ
        else config.DEFAULT_STORM_MARGIN_S
    )

    image_repo_raw = environ.get("IMAGE_REPO", "")
    if image_repo_raw:
        if not Path(image_repo_raw).is_dir():
            raise config.ConfigError(f"IMAGE_REPO not a directory: {image_repo_raw}")
        repo = image_repo_raw
    else:
        repo = ""

    env = {
        "WORKLOAD": profile,
        "IMAGES": str(images),
        "NCPU": str(ncpu),
        "BUILD_PROFILE": build_profile,
        "SCAN_REPEATS": str(scan_repeats),
        "REPEATS": str(hyperfine_repeats),
        "WARMUP": str(warmup),
    }
    if profile.startswith("storm-"):
        env.update(
            {
                "STORM_CLIENTS": str(storm_clients),
                "STORM_OPS": str(storm_ops),
                "STORM_MARGIN_S": str(storm_margin),
            }
        )
    if profile == "upload":
        upload_files = (
            config.integer(environ, "UPLOAD_FILES", images, minimum=1)
            if "UPLOAD_FILES" in environ
            else images
        )
        env["UPLOAD_FILES"] = str(upload_files)

    return Cell(
        level="cell",
        profile=profile,
        images=images,
        repo=repo,
        ncpu=ncpu,
        env=env,
        outdir_suffix=f"cell-{profile}",
    )


def battery_cells(level: str, environ=None) -> tuple:
    """Return the ordered cell list for a battery level."""
    if level not in _KNOWN_LEVELS:
        raise KeyError(level)
    if level == "cell":
        return (_cell_from_env(environ if environ is not None else os.environ),)
    spec = _BATTERIES[level]
    if "scales" in spec:
        scales = spec["scales"]
    else:
        scales = ((spec["images"], spec["repo"]),)
    profiles = spec["profiles"]
    multi = len(scales) > 1
    cells = []
    for images, repo in scales:
        for profile in profiles:
            if multi:
                cells.append(
                    _cell(
                        level,
                        profile,
                        images,
                        repo,
                        cell_name=f"{level}-{profile}-{images}",
                        outdir_suffix=f"{level}-{profile}-{images}",
                    )
                )
            else:
                cells.append(_cell(level, profile, images, repo))
    return tuple(cells)


def repo_path(cell: Cell, sets_root: Path = DEFAULT_SETS_ROOT) -> Path:
    repo = Path(cell.repo)
    return repo if repo.is_absolute() else Path(sets_root) / repo


# --- result rows -------------------------------------------------------------
def _fmt(value) -> str:
    if value is None or value == "" or value == NA:
        return NA
    return str(value)


def _read_manifest(outdir: Path) -> dict:
    try:
        return json.loads((outdir / "run-manifest.json").read_text())
    except OSError, json.JSONDecodeError:
        return {}


def _parse_upload_p50(text: str) -> tuple:
    matches = _UPLOAD_P50.findall(text)
    if not matches:
        return NA, NA
    submit, visible = matches[-1]
    return submit, visible


def build_row(level: str, profile: str, outdir: Path, cell_id: str | None = None) -> dict:
    """Flatten one cell outdir into the run-report row schema.

    The row carries the report columns plus `stages`, the per-scan drain
    rows parsed from `server.log`, `samples`, the values behind the matrix's
    p95/max cells, and `storms`, the per-run storm summaries. Report writers
    iterate `REPORT_COLUMNS`, so none of those helper keys reaches the TSV.
    """
    outdir = Path(outdir)
    extractor = extract_bench
    parsed = extractor.parse_log(outdir, profile=profile)
    status = extractor.parse_status(outdir)
    cell = extractor._cell(parsed, status)
    manifest = _read_manifest(outdir)
    try:
        log_text = (outdir / "harness-a.log").read_text()
    except OSError:
        log_text = ""
    upload_submit, upload_visible = _parse_upload_p50(log_text)

    walls = parsed.get("scan_runs_ms", [])
    row = {
        "cell": cell_id if cell_id is not None else f"{level}-{profile}",
        "profile": profile,
        "level": level,
        "scale": _fmt(parsed.get("images")),
        "build": _fmt(manifest.get("build_profile")),
        "commit": _fmt(manifest.get("git")),
        "scan_wall_s": _fmt(parsed.get("scan_wall_s")),
        "scan_walls_s": ",".join(str(wall) for wall in walls) if walls else NA,
        "upload_submit_p50_ms": upload_submit,
        "upload_visible_p50_ms": upload_visible,
    }
    for column in (
        LATENCY_COLUMNS
        + STORM_FAILED_COLUMNS
        + STAGE_COLUMNS
        + COUNTER_COLUMNS
        + METRICS_COLUMNS
        + SCROLL_COLUMNS
    ):
        row[column] = _fmt(cell.get(column))
    for column in HEADLINE_RANGE_COLUMNS:
        value = row.get(column, NA)
        row[f"{column}_min"] = value
        row[f"{column}_max"] = value
    # The drain parse behind the stage columns, kept whole for the extract
    # document: it comes from the same server.log read, so a cell carries
    # its stage rows without a separate extract run.
    row["stages"] = parsed.get("stages") or []
    # The samples behind the p95/max cells and the per-run storm summaries
    # behind the storm columns, kept for the `:median` row's pooling; they
    # are never written to the TSV.
    row["samples"] = parsed.get("samples") or {}
    row["storms"] = parsed.get("storms") or []
    return row


_NON_NUMERIC_COLUMNS = {
    "cell",
    "profile",
    "level",
    "scale",
    "build",
    "commit",
}


def _sample_key(column: str) -> str:
    """Report column -> the `samples` key holding its raw values."""
    for suffix in ("_p95_ms", "_max_ms"):
        if column.endswith(suffix):
            return column[: -len(suffix)]
    raise KeyError(column)


# Latency columns whose tails pool raw samples across repeats: the idle
# baselines, whose export records every timed run. Probe conditions keep the
# per-rep values the rounds recorded, and storm conditions pool per-run
# summaries.
_SAMPLED_TAIL_COLUMNS = {
    column
    for column in LATENCY_COLUMNS
    if column.startswith("idle_")
    and "_storm_" not in column
    and column.endswith(("_p95_ms", "_max_ms"))
}

_STORM_COLUMN = re.compile(r"^(idle|scan)_storm_(read|thumb|edit)_(p50|p95|max)_ms$")
_STORM_STAT_FIELD = {"p50": "median", "p95": "p95", "max": "max"}


def _storm_runs(rows: list, column: str) -> list:
    """Per-run storm summaries behind one report column, across all repeats."""
    match = _STORM_COLUMN.match(column)
    if match is None:
        return []
    background, report_kind, stat = match.groups()
    kind = extract_bench.STORM_OP_KIND[report_kind]
    values = []
    for row in rows:
        for storm in row.get("storms") or []:
            if (
                extract_bench.storm_background(storm) == background
                and extract_bench.storm_kind(storm) == kind
            ):
                values.append(storm[_STORM_STAT_FIELD[stat]] * 1000.0)
    return values


def _numeric_values(raw) -> list:
    """Parse one report cell as a list of numbers.

    Repeat rows carry per-scan/per-run lists (`scan_walls_s`, stage
    p50/max) as comma-separated text, so one cell contributes every
    member. Empty entries, `n/a` and unparseable text contribute nothing.
    """
    if raw is None:
        return []
    values = []
    for part in str(raw).split(","):
        part = part.strip()
        if part == "" or part == NA:
            continue
        try:
            values.append(float(part))
        except ValueError:
            continue
    return values


def _build_median_row(rows: list) -> dict:
    """Return one median row for the given repeat rows.

    Comma-separated lists are pooled across repeats and the median taken
    over every value (median of medians would weight a two-scan cell as
    one sample per repeat).

    Three column families aggregate differently:

    - matrix `p50` columns keep that pooling (median over the repeats),
      which is how the recorded baselines were produced;
    - `idle_*` `p95`/`max` columns pool the timed post-scan runs every rep
      carries and take the percentile or worst over all of them, so a tail
      describes every run rather than the median of three tails;
    - storm `p95`/`max` columns pool the per-run summaries and take the
      worst; failed counts are summed, because a count across repeats is a
      total.

    Probe `p95`/`max` columns follow the default pooling: each rep already
    summarises its probe rounds, and the recorded worst-case baselines were
    medians of those per-rep values.
    """
    if not rows:
        return {}
    first = dict(rows[0])
    base_cell = first.get("cell", "")
    median_cell = base_cell + ":median" if isinstance(base_cell, str) and base_cell else "median"
    result = dict(first)
    for column in REPORT_COLUMNS:
        if column in _NON_NUMERIC_COLUMNS:
            # Non-numeric columns pass through from the first repeat.
            result[column] = first.get(column, result.get(column, NA))
            continue
        if column.endswith("_min"):
            base = column[:-4]
            values = []
            for row in rows:
                values.extend(_numeric_values(row.get(column, row.get(base, NA))))
            result[column] = _fmt(min(values)) if values else NA
            continue
        if column.endswith("_max"):
            base = column[:-4]
            values = []
            for row in rows:
                values.extend(_numeric_values(row.get(column, row.get(base, NA))))
            result[column] = _fmt(max(values)) if values else NA
            continue
        if column.endswith("_failed"):
            values = []
            for row in rows:
                values.extend(_numeric_values(row.get(column, NA)))
            result[column] = _fmt(int(sum(values))) if values else NA
            continue
        if column in _SAMPLED_TAIL_COLUMNS:
            pooled = []
            for row in rows:
                pooled.extend((row.get("samples") or {}).get(_sample_key(column), []))
            if pooled:
                value = (
                    max(pooled)
                    if column.endswith("_max_ms")
                    else extract_bench.percentile(pooled, 95)
                )
                result[column] = _fmt(round(value, 1))
                continue
            values = []
            for row in rows:
                values.extend(_numeric_values(row.get(column, NA)))
            result[column] = _fmt(round(max(values), 6)) if values else NA
            continue
        if _STORM_COLUMN.match(column):
            runs = _storm_runs(rows, column)
            if runs:
                value = statistics.median(runs) if column.endswith("_p50_ms") else max(runs)
                result[column] = _fmt(round(value, 1))
                continue
            values = []
            for row in rows:
                values.extend(_numeric_values(row.get(column, NA)))
            result[column] = _fmt(round(max(values), 6)) if values else NA
            continue
        values = []
        for row in rows:
            values.extend(_numeric_values(row.get(column, NA)))
        # Round out binary-float noise from averaging two odd-count
        # neighbours (ms/1000 seconds land as 6.657500000000001); every
        # report column resolves well above 1e-6.
        result[column] = _fmt(round(statistics.median(values), 6)) if values else NA
    result["cell"] = median_cell
    result.pop("samples", None)
    result.pop("storms", None)
    return result


def _row_line(row: dict) -> str:
    return "\t".join(
        str(row.get(column, NA)).replace("\t", " ").replace("\n", " ") for column in REPORT_COLUMNS
    )


def write_report(path: Path, rows: list) -> None:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    lines = ["\t".join(REPORT_COLUMNS)] + [_row_line(row) for row in rows]
    path.write_text("\n".join(lines) + "\n")


def append_row(path: Path, row: dict) -> None:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        path.write_text("\t".join(REPORT_COLUMNS) + "\n")
    with path.open("a", encoding="utf-8") as handle:
        handle.write(_row_line(row) + "\n")


def load_report(path: Path) -> list:
    """Read a run report; returns one dict per data row."""
    lines = [line for line in Path(path).read_text().splitlines() if line.strip()]
    if not lines:
        raise ValueError(f"{path}: empty report")
    header = lines[0].split("\t")
    if header != REPORT_COLUMNS:
        raise ValueError(f"{path}: unexpected report header")
    rows = []
    for line in lines[1:]:
        if line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != len(header):
            raise ValueError(f"{path}: malformed row with {len(parts)} columns: {line}")
        rows.append(dict(zip(header, parts, strict=True)))
    return rows


# --- battery-level JSON extract ---------------------------------------------
# Identity subset of run-manifest.json: output key -> candidate manifest keys.
_MANIFEST_IDENTITY = (
    ("dataset_id", ("dataset_id",)),
    ("dataset_count", ("dataset_count",)),
    ("commit", ("git", "commit")),
    ("build_profile", ("build_profile",)),
    ("observer", ("observer_stats", "effective_observer_stats", "PICASU_OBSERVER_STATS")),
    ("mode", ("mode",)),
    ("binary_sha", ("binary_sha", "binary_sha256", "server_binary_sha256")),
)

_REP_DIR_RE = re.compile(r"^(?P<cell>.+)-rep(?P<rep>\d+)$")

# Entries a battery parent may legitimately hold besides `*-repN` subdirs.
# `report.md` and `matrix.tsv` are the extract's human-readable views of
# the parent's medians; `environment.json` is the host snapshot taken when
# the battery started (and refreshed when it finished).
_BATTERY_PARENT_ALLOWED_FILES = {
    "report.tsv",
    "report.json",
    "orchestrator.log",
    "report.md",
    "matrix.tsv",
    "environment.json",
}


def foreign_entries(parent) -> list:
    """Names of entries in `parent` that are not battery-created.

    An empty list means the directory is either absent or a clean battery
    parent (only `*-repN` subdirs plus the known report/log files).
    """
    parent = Path(parent)
    if not parent.exists() or not parent.is_dir():
        return []
    foreign = []
    for entry in parent.iterdir():
        name = entry.name
        if entry.is_dir() and _REP_DIR_RE.match(name):
            continue
        if entry.is_file() and name in _BATTERY_PARENT_ALLOWED_FILES:
            continue
        foreign.append(name)
    return sorted(foreign)


def validate_parent(parent, *, fresh: bool = False) -> list:
    """Validate (and, conceptually, gate) a battery parent directory.

    Returns the (possibly empty) list of foreign entries. Raises
    `ConfigError` when `parent` exists, is not a directory, or holds
    foreign content while `fresh` is False. `fresh=True` is the explicit
    escape hatch: callers wipe first and this function reports what was
    there without raising.
    """
    parent = Path(parent)
    if not parent.exists():
        return []
    if not parent.is_dir():
        raise ConfigError(f"{parent}: not a battery directory (not a directory)")
    foreign = foreign_entries(parent)
    if foreign and not fresh:
        raise ConfigError(
            f"{parent}: refusing to run into a non-battery directory; "
            f"foreign entries: {', '.join(foreign)}"
        )
    return foreign


# Back-compat / alternate names used by callers and tests.
validate_battery_parent = validate_parent
check_parent = validate_parent


def is_battery_parent(parent) -> bool:
    return not foreign_entries(parent)


def _rep_dirs(parent: Path) -> list:
    """Sorted `<level>-<profile>-repN` subdirectories of one battery parent."""
    found = []
    for entry in parent.iterdir():
        if not entry.is_dir():
            continue
        match = _REP_DIR_RE.match(entry.name)
        if match:
            found.append((match.group("cell"), int(match.group("rep")), entry))
    # Natural order: cells lexicographically, rep numbers numerically
    # (`-rep10` must not sort between `-rep1` and `-rep2`).
    return [entry for _cell, _rep, entry in sorted(found, key=lambda item: item[:2])]


def _manifest_identity(outdir: Path) -> dict:
    """The run-manifest identity subset; absent keys become JSON nulls."""
    manifest = _read_manifest(outdir)
    return {
        name: next(
            (manifest[key] for key in sources if manifest.get(key) is not None),
            None,
        )
        for name, sources in _MANIFEST_IDENTITY
    }


def _read_stages(outdir: Path) -> list:
    """Parsed `stages.json` content; an absent or corrupt file yields [].

    `stages.json` is written by a standalone `extract_bench.py` run, so it
    only exists for cells that were extracted by hand; the extract entry
    prefers the drain rows `build_row` parsed from `server.log` and uses
    this as the fallback for the rest.
    """
    try:
        stages = json.loads((outdir / "stages.json").read_text())
    except OSError, json.JSONDecodeError:
        return []
    return stages if isinstance(stages, list) else []


def _terminal_state(outdir: Path) -> str:
    """Terminal server state from `status-final.json`, else "incomplete"."""
    try:
        status = json.loads((outdir / "status-final.json").read_text())
    except OSError, json.JSONDecodeError:
        return "incomplete"
    state = status.get("state") if isinstance(status, dict) else None
    return state if isinstance(state, str) and state else "incomplete"


def _extract_entry(path: Path) -> dict:
    """One rep subdir: row + manifest + stages + terminal, or an error entry."""
    match = _REP_DIR_RE.match(path.name)
    cell = match.group("cell") if match else ""
    level = None
    remainder = ""
    for candidate in _KNOWN_LEVELS:
        if cell == candidate:
            level = candidate
            remainder = ""
            break
        if cell.startswith(candidate + "-"):
            level = candidate
            remainder = cell[len(candidate) + 1 :]
            break
    if level is None:
        return {
            "dir": path.name,
            "error": "cannot derive level and profile from directory name",
        }
    profile = re.sub(r"-\d+$", "", remainder) if remainder else remainder
    if not profile:
        profile = "control" if level == "micro" else profile
    if not profile:
        return {
            "dir": path.name,
            "error": "cannot derive level and profile from directory name",
        }
    try:
        row = build_row(level, profile, path, cell_id=cell)
    except (Exception, SystemExit) as err:
        # Broken reps become error entries so one bad dir cannot abort the
        # walk; KeyboardInterrupt still aborts the extract.
        if isinstance(err, SystemExit):
            message = f"extractor exited with code {err.code}"
        else:
            message = str(err) or type(err).__name__
        return {"dir": path.name, "error": message}
    # The row's own drain parse first (the server.log the report's stage
    # columns came from), then a hand-written stages.json for cells the
    # standalone extractor summarised instead. The row stays report-shaped.
    stages = row.pop("stages", None) or _read_stages(path)
    return {
        "dir": path.name,
        "row": row,
        "manifest": _manifest_identity(path),
        "stages": stages,
        "terminal": _terminal_state(path),
    }


def _report_row(row: dict) -> dict:
    """The report-shaped subset of a built row (helper keys dropped)."""
    return {column: row.get(column, NA) for column in REPORT_COLUMNS}


def _median_rows(rows: list) -> list:
    """One `<cell>:median` row per cell that contributed repeat rows.

    Mirrors the report, which appends a median row only for cells that
    ran more than once; the pooling itself is `_build_median_row`'s.
    """
    groups: dict = {}
    for row in rows:
        groups.setdefault(row.get("cell", ""), []).append(row)
    return [_build_median_row(group) for group in groups.values() if len(group) > 1]


def _json_safe(value):
    """Recursively replace non-finite floats so `json.dump` stays strict.

    `n/a` is a string in report rows and passes through untouched.
    """
    if isinstance(value, float) and not math.isfinite(value):
        return None
    if isinstance(value, dict):
        return {key: _json_safe(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_json_safe(item) for item in value]
    return value


# --- battery-level matrix views ---------------------------------------------
# Human-readable views over the extract's median rows: one table per
# statistic, rows = accesses, columns = backgrounds. Every cell rolls up
# the profile medians that measured it (median across the contributors,
# contributor count recorded next to it); a cell no profile measured is
# `n/a`. The views are derived here, never stored in the JSON document —
# the document's `medians` carry everything they recompute from.
# `matrix.tsv` is the spreadsheet-readable variant of the same values;
# `report.md` is the human report built on top of them.
MATRIX_BACKGROUNDS = tuple(extract_bench.BACKGROUNDS)
MATRIX_ACCESSES = tuple(extract_bench.ACCESSES)
MATRIX_STATS = tuple(extract_bench.STATS)
MATRIX_FILE = "matrix.tsv"
REPORT_FILE = "report.md"


def matrix_rollup(median_rows: list) -> dict:
    """Roll the extract's median rows up into the battery-level view cells.

    Returns `{(stat, background, access): (value, count)}` over every
    background x access x statistic column of the median rows. `value`
    is the median across the contributing profile medians (a lone
    contributor passes its recorded string through, no contributor is
    `n/a`) and `count` is how many median rows contributed. Non-numeric,
    `n/a` and non-finite values never contribute.
    """
    contributions: dict = {}
    for row in median_rows:
        for stat in MATRIX_STATS:
            for background in MATRIX_BACKGROUNDS:
                for access in MATRIX_ACCESSES:
                    raw = row.get(f"{background}_{access}_{stat}_ms")
                    if raw is None:
                        continue
                    text = str(raw).strip()
                    if text in ("", NA):
                        continue
                    try:
                        value = float(text)
                    except ValueError:
                        continue
                    if not math.isfinite(value):
                        continue
                    contributions.setdefault((stat, background, access), []).append((value, text))
    rollup: dict = {}
    for stat in MATRIX_STATS:
        for background in MATRIX_BACKGROUNDS:
            for access in MATRIX_ACCESSES:
                found = contributions.get((stat, background, access), [])
                if not found:
                    rollup[(stat, background, access)] = (NA, 0)
                elif len(found) == 1:
                    rollup[(stat, background, access)] = (found[0][1], 1)
                else:
                    median = statistics.median(value for value, _text in sorted(found))
                    rollup[(stat, background, access)] = (str(round(median, 6)), len(found))
    return rollup


def render_matrix(median_rows: list) -> str:
    """Render the rollup as one TSV document of four sections.

    Sections are separated by a blank line and introduced by a `#`
    comment: the three statistic matrices (rows = the six accesses,
    columns = the three backgrounds, values in ms) followed by the
    per-cell contributor counts.
    """
    rollup = matrix_rollup(median_rows)
    blocks = []
    for stat in MATRIX_STATS:
        lines = [f"# {stat} (ms)", "\t".join(("access",) + MATRIX_BACKGROUNDS)]
        for access in MATRIX_ACCESSES:
            values = [rollup[(stat, background, access)][0] for background in MATRIX_BACKGROUNDS]
            lines.append("\t".join((access, *values)))
        blocks.append("\n".join(lines))
    lines = ["# contributors", "\t".join(("stat", "access") + MATRIX_BACKGROUNDS)]
    for stat in MATRIX_STATS:
        for access in MATRIX_ACCESSES:
            counts = [
                str(rollup[(stat, background, access)][1]) for background in MATRIX_BACKGROUNDS
            ]
            lines.append("\t".join((stat, access, *counts)))
    blocks.append("\n".join(lines))
    return "\n\n".join(blocks) + "\n"


# Report access name each storm profile's burst fills (the extractor's
# report columns; `storm-idle` bursts with IDLE_PROFILE, default read).
_STORM_ACCESS = {
    "storm-read": "storm_read",
    "storm-thumb": "storm_thumb",
    "storm-diff": "storm_edit",
    "storm-idle": "storm_read",
}
_MATRIX_COLUMN_RE = re.compile(
    r"^(idle|scan|uscan)_(prefetch|thumb|edit|storm_read|storm_thumb|storm_edit)"
    r"_(p50|p95|max)_ms$"
)


def expected_columns(profile: str) -> list:
    """Report columns one profile's median row is expected to fill.

    The expectations come from the workload design: during-scan probes
    land in the cell's own background (`scan`, or `uscan` for upload), a
    storm profile measures exactly its own burst condition, every profile
    records the idle post-scan baselines and the observer drain stages,
    and the upload profile records its submit/visible p50s.
    """
    probes = ("prefetch", "thumb", "edit")
    columns = []
    if profile == "control":
        columns += [f"scan_{access}_{stat}_ms" for access in probes for stat in MATRIX_STATS]
    elif profile == "upload":
        columns += [f"uscan_{access}_{stat}_ms" for access in probes for stat in MATRIX_STATS]
    elif profile.startswith("storm-"):
        access = _STORM_ACCESS.get(profile)
        if access:
            background = "idle" if profile == "storm-idle" else "scan"
            columns += [f"{background}_{access}_{stat}_ms" for stat in MATRIX_STATS]
            columns += [f"{background}_{access}_failed"]
    columns += [f"idle_{access}_{stat}_ms" for access in probes for stat in MATRIX_STATS]
    if profile == "upload":
        columns += ["upload_submit_p50_ms", "upload_visible_p50_ms"]
    columns += list(STAGE_COLUMNS)
    return columns


def _report_header(document: dict) -> list:
    """The identity and outcome lines of `report.md`: battery, level,
    scale, build, commit, date, cell outcomes and any failures."""
    medians = document.get("medians") or []
    cells = document.get("cells") or []

    def first(key: str) -> str:
        rows = medians + [entry["row"] for entry in cells if "row" in entry]
        return next(
            (str(row.get(key)) for row in rows if row.get(key) not in (None, "", NA)),
            NA,
        )

    counts: dict = {"completed": 0, "failed": 0, "incomplete": 0}
    failures = []
    errors = 0
    for entry in cells:
        if "error" in entry:
            errors += 1
            failures.append(f"{entry['dir']}: error ({entry['error']})")
            continue
        state = entry.get("terminal") or "incomplete"
        counts[state] = counts.get(state, 0) + 1
        if state != "completed":
            failures.append(f"{entry['dir']}: {state}")
    level = document.get("level")
    outcomes = ", ".join(f"{name}={counts[name]}" for name in counts)
    return [
        f"- battery: `{document.get('battery', '')}`",
        f"- level: `{level if isinstance(level, str) and level else NA}`",
        f"- scale: `{first('scale')}`",
        f"- build: `{first('build')}`",
        f"- commit: `{first('commit')}`",
        f"- date: `{date.today().isoformat()}`",
        f"- outcomes: {outcomes}, errors={errors}",
        f"- failures: {'; '.join(failures) if failures else 'none'}",
    ]


def _report_anomalies(document: dict, rollup: dict) -> list:
    """The anomalies section, generated from the data: the worst single
    value per stat (with its cell), every `n/a` a profile was expected
    to fill, and expected cells whose rollup rests on fewer than two
    contributor medians. Unmeasured-by-design cells are not anomalies.
    """
    medians = document.get("medians") or []
    lines = ["## Anomalies", "", "### worst values", ""]
    worst_rows = []
    for stat in MATRIX_STATS:
        best = None
        for row in medians:
            for background in MATRIX_BACKGROUNDS:
                for access in MATRIX_ACCESSES:
                    raw = row.get(f"{background}_{access}_{stat}_ms")
                    if raw is None:
                        continue
                    text = str(raw).strip()
                    try:
                        value = float(text)
                    except ValueError:
                        continue
                    if not math.isfinite(value):
                        continue
                    if best is None or value > best[0]:
                        best = (value, text, str(row.get("cell", NA)), background, access)
        if best:
            _value, text, cell, background, access = best
            worst_rows.append(f"| {stat} | {text} | {cell} | {background}/{access} |")
    if worst_rows:
        lines += [
            "| stat | value (ms) | cell | condition |",
            "| --- | --- | --- | --- |",
            *worst_rows,
        ]
    else:
        lines.append("- none")

    lines += ["", "### expected data missing", ""]
    missing = []
    for row in medians:
        profile = str(row.get("profile", ""))
        for column in expected_columns(profile):
            if row.get(column) in (None, "", NA):
                missing.append(f"{row.get('cell', NA)}: {column} = {NA}")
    lines += [f"- {item}" for item in missing] if missing else ["- none"]

    lines += ["", "### contributor counts below 2", ""]
    profiles = []
    for row in medians:
        profile = str(row.get("profile", ""))
        if profile and profile not in profiles:
            profiles.append(profile)
    # Only the shared `idle_*` probe/baseline columns are expected from more
    # than one contributor: every profile fills them, so a count below 2 means
    # profiles stopped reporting. Structurally single-contributor columns
    # (`scan_*`, `uscan_*`, and the storm kinds) are by design and never
    # flagged, even when their count is 1 (or 0).
    expected_counts: dict = {}
    grid = set()
    for profile in profiles:
        for column in expected_columns(profile):
            match = _MATRIX_COLUMN_RE.match(column)
            if match is None:
                continue
            background, access = match.group(1), match.group(2)
            if background != "idle" or access not in ("prefetch", "thumb", "edit"):
                continue
            grid.add((background, access))
            expected_counts[(background, access)] = expected_counts.get((background, access), 0) + 1
    thin = [
        (background, access)
        for background in MATRIX_BACKGROUNDS
        for access in MATRIX_ACCESSES
        if (background, access) in grid
        and expected_counts.get((background, access), 0) > 1
        and any(rollup[(stat, background, access)][1] < 2 for stat in MATRIX_STATS)
    ]
    if thin:
        lines += [
            "| background | access | p50 | p95 | max |",
            "| --- | --- | --- | --- | --- |",
        ]
        for background, access in thin:
            counts = [str(rollup[(stat, background, access)][1]) for stat in MATRIX_STATS]
            lines.append(f"| {background} | {access} | " + " | ".join(counts) + " |")
    else:
        lines.append("- none")
    return lines


def _report_environment(environment_record) -> list:
    """The run-host section of `report.md`: what machine produced these rows."""
    if not isinstance(environment_record, dict):
        return ["- environment: not recorded (run predates environment capture)"]
    get = environment_record.get
    dirty = " (dirty tree)" if get("git_dirty") is True else ""
    return [
        f"- host: `{get('hostname', NA)}`",
        f"- kernel: `{get('kernel', NA)}`",
        f"- cpu: `{get('cpu_model', NA)}` x{get('cpu_count', NA)}",
        f"- freq: `{get('freq_cur_mhz', NA)}` MHz cur / `{get('freq_max_mhz', NA)}` MHz max (`{get('governor', NA)}`)",
        f"- memory: `{get('mem_total_kb', NA)}` kB",
        f"- load at start: `{get('load_1', NA)}` `{get('load_5', NA)}` `{get('load_15', NA)}`",
        f"- load at end: `{get('end_load_1', NA)}` `{get('end_load_5', NA)}` `{get('end_load_15', NA)}`",
        f"- started: `{get('started_utc', NA)}`, ended: `{get('ended_utc', NA)}`",
        f"- commit: `{get('git_commit', NA)}`{dirty}",
    ]


def render_report(document: dict) -> str:
    """Render `report.md`: header, the three stat matrices with the
    contributor counts, and anomalies.

    Every section is generated from the extract's own rows — ranked
    facts only, no prose judgments — and deterministic for one input.
    The matrix values come from the same rollup as `matrix.tsv`.
    """
    medians = document.get("medians") or []
    rollup = matrix_rollup(medians)
    lines = ["# Battery report", ""]
    lines += _report_header(document)
    lines += ["", "## Environment", ""]
    lines += _report_environment(document.get("environment"))
    for stat in MATRIX_STATS:
        lines += [
            "",
            f"## {stat} (ms)",
            "",
            "| access | idle | scan | uscan |",
            "| --- | --- | --- | --- |",
        ]
        for access in MATRIX_ACCESSES:
            values = [rollup[(stat, background, access)][0] for background in MATRIX_BACKGROUNDS]
            lines.append("| " + " | ".join((access, *values)) + " |")
    lines += [
        "",
        "## contributors",
        "",
        "| stat | access | idle | scan | uscan |",
        "| --- | --- | --- | --- | --- |",
    ]
    for stat in MATRIX_STATS:
        for access in MATRIX_ACCESSES:
            counts = [
                str(rollup[(stat, background, access)][1]) for background in MATRIX_BACKGROUNDS
            ]
            lines.append("| " + " | ".join((stat, access, *counts)) + " |")
    lines += [""] + _report_anomalies(document, rollup)
    return "\n".join(lines) + "\n"


def _git_branch(commit: str = "") -> str:
    """Branch holding `commit`, else the current checkout's branch."""
    if commit:
        try:
            result = subprocess.run(
                ["git", "name-rev", "--name-only", commit],
                capture_output=True,
                text=True,
                cwd=str(SCRIPT_DIR),
            )
        except OSError:
            result = None
        if result is not None and result.returncode == 0:
            name = result.stdout.strip()
            if name and not name.startswith("undefined"):
                for prefix in ("remotes/origin/", "remotes/", "tags/"):
                    name = name.removeprefix(prefix)
                return name
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--abbrev-ref", "HEAD"],
            capture_output=True,
            text=True,
            cwd=str(SCRIPT_DIR),
        )
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip()
    except OSError:
        pass
    return "unknown"


_DEFAULT_BASELINE_COMMENTS = [
    "# Accepted benchmark values for the micro and gate batteries.",
    "#",
    "# `battery check` compares `<cell>:median` rows only. Long format:",
    "#   cell metric baseline recorded note",
]


def _baseline_header(existing_path) -> tuple:
    """Comment+header block and raw data rows of an existing baselines file."""
    header = []
    rows = {}
    cells = []
    if existing_path and Path(existing_path).is_file():
        data = False
        for line in Path(existing_path).read_text().splitlines():
            if not data:
                header.append(line)
                if line.startswith("cell\tmetric\t"):
                    data = True
                continue
            if not line.strip():
                continue
            parts = line.split("\t")
            if len(parts) != len(BASELINE_COLUMNS):
                raise ValueError(f"{existing_path}: malformed baselines row: {line}")
            rows[(parts[0], parts[1])] = parts
            if parts[0] not in cells:
                cells.append(parts[0])
    if not data:
        header = _DEFAULT_BASELINE_COMMENTS + [BASELINE_HEADER]
    return "\n".join(header) + "\n", rows, cells


def _baseline_value(value) -> str:
    """Report value in the file's one-decimal style (8 -> 8.0)."""
    try:
        return str(float(value))
    except TypeError, ValueError:
        return str(value)


def render_baselines_tsv(medians, existing_path, *, branch, commit, run_date):
    """Render the merged baselines.tsv long format from pooled median rows.

    Every emitted cell+metric keeps its accepted `note`; `baseline` and
    `recorded` move to this run. Rows for cells absent from this battery
    are carried over unchanged, so extracting the micro and gate batteries
    in sequence builds one complete file.

    Returns `(text, new_metrics)`, where `new_metrics` lists the pairs that
    had no accepted row.
    """
    header, existing, existing_cells = _baseline_header(existing_path)
    emitted = {}
    ordered_cells = []
    branches = {commit: branch}
    for m in sorted(medians, key=lambda row: CELL_ORDER.get(row.get("cell", ""), 99)):
        cell = m.get("cell", "")
        if not cell.endswith(":median"):
            cell = f"{cell}:median"
        if cell not in ordered_cells:
            ordered_cells.append(cell)
        row_commit = m.get("commit") or commit
        if row_commit not in branches:
            branches[row_commit] = _git_branch(row_commit)
        recorded = f"{run_date} {branches[row_commit]} {row_commit}".strip()
        for metric in BASELINE_METRICS:
            value = m.get(metric)
            if value in (None, "", NA):
                continue
            emitted[(cell, metric)] = (_baseline_value(value), recorded)
    cells = list(dict.fromkeys(existing_cells + ordered_cells))
    extras = {}
    for cell, metric in existing:
        if metric not in BASELINE_METRICS:
            extras.setdefault(cell, []).append(metric)
    new_metrics = []
    lines = []
    for cell in cells:
        for metric in BASELINE_METRICS + tuple(extras.get(cell, ())):
            key = (cell, metric)
            if key in emitted:
                value, recorded = emitted[key]
                accepted = existing.get(key)
                if accepted is None:
                    note = "new metric recorded " + run_date
                    new_metrics.append(key)
                else:
                    note = accepted[4]
                lines.append(f"{cell}\t{metric}\t{value}\t{recorded}\t{note}")
            elif key in existing:
                lines.append("\t".join(existing[key]))
    text = header + "\n".join(lines) + ("\n" if lines else "")
    return text, new_metrics


def _distinct(values) -> str:
    def key(value):
        return tuple(
            int(part) if part.isdigit() else part for part in re.split(r"(\d+)", str(value))
        )

    return ",".join(
        sorted({str(value) for value in values if value not in (None, "", NA)}, key=key)
    )


def _manifest_field(manifest: dict, name: str):
    for key in dict(_MANIFEST_IDENTITY)[name]:
        if manifest.get(key) is not None:
            return manifest[key]
    return None


def _run_date(paths) -> str:
    dates = []
    for path in paths:
        started = _read_manifest(path).get("started")
        if isinstance(started, str) and len(started) >= 10:
            dates.append(started[:10])
    return min(dates) if dates else date.today().isoformat()


def _resolve_parents(raw_paths):
    """Expand CLI paths to battery parents: each path is either a battery
    parent itself or a run root whose immediate subdirectories are. Empty
    and foreign directories are skipped; a path yielding nothing is an error."""
    found = []
    for raw in raw_paths:
        parent = Path(raw)
        if not parent.is_dir():
            return [], f"{parent}: not a battery directory"
        if is_battery_parent(parent):
            if any(parent.iterdir()):
                found.append(parent)
            continue
        contained = sorted(
            [
                s
                for s in parent.iterdir()
                if s.is_dir() and is_battery_parent(s) and any(s.iterdir())
            ],
            key=lambda s: s.name,
        )
        if not contained:
            return [], f"{parent}: no *-repN subdirectories and no contained batteries"
        found.extend(contained)
    if not found:
        return [], "no batteries found"
    return found, None


def _env_key(record: dict) -> tuple:
    """Identity of an environment record: same run, same key."""
    commit = record.get("commit") or record.get("git_commit", "")
    return (str(commit), str(record.get("level", "")), str(record.get("started_utc", "")))


def _synthetic_env(parent, paths, rows) -> dict:
    """Environment stand-in for batteries run before env capture existed.

    Level/scale/commit/started come from the run's own artifacts; the
    machine fields stay unknown rather than inventing data.
    """
    from datetime import date

    levels = sorted({str(r.get("level", "")) for r in rows if r.get("level")})
    scales = sorted(
        {str(r.get("scale", "")) for r in rows if r.get("scale")},
        key=lambda v: (0, int(v)) if v.isdigit() else (1, v),
    )
    commits = [str(r.get("commit", "")) for r in rows if r.get("commit")]
    commit = commits[0] if commits else ""
    started = None
    for path in paths:
        stamp = _read_manifest(path).get("started")
        if isinstance(stamp, str) and len(stamp) >= 10 and (started is None or stamp < started):
            started = stamp
    # Machine fields stay unknown: the run predates capture, and
    # guessing them would be lying. The md intro says what unknown means.
    unknown = "unknown"
    return {
        "level": ",".join(levels),
        "scale": ",".join(scales),
        "started_utc": started if started is not None else date.today().isoformat(),
        "ended_utc": environment.UNKNOWN,
        "host": unknown,
        "kernel": unknown,
        "cpu_model": unknown,
        "cpu_count": unknown,
        "freq_cur_mhz": unknown,
        "freq_max_mhz": unknown,
        "governor": unknown,
        "turbo_disabled": unknown,
        "mem_total_kb": unknown,
        "load_1": unknown,
        "load_5": unknown,
        "load_15": unknown,
        "end_load_1": unknown,
        "end_load_5": unknown,
        "end_load_15": unknown,
        "git_branch": _git_branch(commit),
        "git_commit": commit,
        "git_dirty": unknown,
    }


def merge_env_lines(existing_lines, records) -> list:
    """Merge environment JSON lines: same run key replaces, else append."""
    import json

    merged = []
    seen = set()
    for line in existing_lines:
        try:
            record = json.loads(line[len("# env:") :])
            key = _env_key(record)
        except ValueError, AttributeError:
            merged.append(line)
            continue
        seen.add(key)
        merged.append(line)
    for record in records:
        key = _env_key(record)
        line = "# env: " + json.dumps(record, sort_keys=True)
        if key not in seen:
            merged.append(line)
            seen.add(key)
            continue
        for i, existing in enumerate(merged):
            try:
                same = _env_key(json.loads(existing[len("# env:") :])) == key
            except ValueError:
                continue
            if same:
                merged[i] = line
                break
    return merged


def record_baselines(parents, tsv_path, merge_path=None, md_path=None) -> int:
    """Merge pooled medians from battery parents and optionally render the doc.

    One step: extract every parent, union all `:median` rows into the
    accepted-values file (existing notes kept, baseline+recorded move to
    the extracting run), then — with `md_path` — render the full
    `baselines.md` from the merged values plus every parent's `report.tsv`. Cells absent
    from every parent are carried over unchanged.
    """
    from render_baselines import load_baseline_values, load_reports, render

    parents, error = _resolve_parents(parents)
    if error is not None:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    rep_dirs: list = []
    medians: list = []
    env_records: list = []
    for parent in parents:
        paths = _rep_dirs(parent)
        if not paths:
            print(f"ERROR: {parent}: no *-repN subdirectories", file=sys.stderr)
            return 1
        rows = []
        for path in paths:
            entry = _extract_entry(path)
            if "error" not in entry:
                rows.append(entry["row"])
        if not rows:
            print(f"ERROR: {parent}: every rep failed to parse", file=sys.stderr)
            return 2
        env_record = environment.load_environment(parent / environment.ENVIRONMENT_FILENAME)
        if env_record is None:
            env_record = _synthetic_env(parent, paths, rows)
        env_records.append(env_record)
        rep_dirs.extend(paths)
        medians.extend(_report_row(row) for row in _median_rows(rows))
    if not medians:
        print("ERROR: no median rows extracted", file=sys.stderr)
        return 2
    if merge_path is not None:
        merge_source = Path(merge_path)
    elif not (isinstance(tsv_path, str) and tsv_path == "-") and Path(tsv_path).is_file():
        merge_source = Path(tsv_path)
    else:
        merge_source = SCRIPT_DIR / "baselines.tsv"
    run_date = _run_date(rep_dirs)
    text, new_metrics = render_baselines_tsv(
        medians,
        merge_source,
        branch="",
        commit="",
        run_date=run_date,
    )
    # Fold the run environments into `# env:` comment lines just above the
    # header row: same run replaces, other runs (e.g. the other level)
    # carry over, so the file records every run behind its baselines.
    import json as _json

    existing_env = [line for line in text.splitlines() if line.startswith("# env:")]
    merged_env_lines = merge_env_lines(existing_env, env_records)
    body = [line for line in text.splitlines() if not line.startswith("# env:")]
    header_at = next(i for i, line in enumerate(body) if line.startswith("cell\t"))
    text = "\n".join(body[:header_at] + merged_env_lines + body[header_at:]) + "\n"
    merged_env = []
    for line in merged_env_lines:
        try:
            record = _json.loads(line[len("# env:") :])
        except ValueError:
            continue
        if isinstance(record, dict):
            merged_env.append(record)
    if md_path is not None:
        covered = {m.get("cell", "").split(":")[0] for m in medians if m.get("cell")}
        baselined = set()
        if merge_source.is_file():
            for line in merge_source.read_text().splitlines():
                parts = line.split("\t")
                if len(parts) == len(BASELINE_COLUMNS) and parts[0] != "cell":
                    baselined.add(parts[0].split(":")[0])
        missing = sorted(baselined - covered)
        if missing:
            print(
                f"warn: baselined cells have no fresh medians here and render as n/a: "
                f"{', '.join(missing)} (record the missing batteries to fill them in)",
                file=sys.stderr,
            )
    reports = []
    for parent in parents:
        report_path = parent / "report.tsv"
        if not report_path.is_file():
            print(f"ERROR: {report_path}: no report.tsv (run the battery first?)", file=sys.stderr)
            return 1
        reports.append(str(report_path))
    to_stdout = tsv_path is True or tsv_path == "-"
    if md_path is not None and to_stdout:
        print(
            "ERROR: --render-md needs a --baselines-tsv file to read the baselines back",
            file=sys.stderr,
        )
        return 2
    if not to_stdout:
        out_path = Path(tsv_path)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(text)
        print(f"wrote baselines TSV: {out_path}", file=sys.stderr)
    else:
        print(text, end="")
    if new_metrics:
        print("new metrics recorded without prior baseline:", file=sys.stderr)
        for cell, metric in new_metrics:
            print(f"  {cell} {metric}", file=sys.stderr)
    if md_path is not None:
        baselines = load_baseline_values(out_path)
        report = load_reports(reports)
        md_out = Path(md_path)
        md_out.parent.mkdir(parents=True, exist_ok=True)
        md_out.write_text(render(report, baselines, merged_env))
        print(f"wrote baselines Markdown: {md_out}", file=sys.stderr)
    return 0


def extract_battery(
    parent: Path,
    out: Path | None = None,
    *,
    matrix_only: bool = False,
    matrix_format: str = "both",
) -> int:
    """Dump one battery parent as a single JSON document.

    Walks the `<level>-<profile>-repN` subdirs in sorted order and emits
    one entry per subdir (row, manifest identity, stages, terminal state;
    a rep whose artifacts no longer parse becomes an `{"dir", "error"}`
    entry), the pooled median rows, the parent's `report.tsv` rows and an
    `errors` count. The pooled medians also render into the human
    artifacts beside the JSON — `report.md` and/or `matrix.tsv`, selected
    by `matrix_format` (md/tsv/both), next to `out`, else inside the
    parent; with `matrix_only` the selected human view reaches stdout
    instead of the JSON document. Exit 0 when at least one rep parsed, 1
    when the parent holds no rep subdirs, 2 when every rep failed to
    parse.

    Band recording lives in `record_baselines`, not here.
    """
    if matrix_format == "both":
        formats = ("md", "tsv")
    elif matrix_format in ("md", "tsv"):
        formats = (matrix_format,)
    else:
        raise ValueError(f"unknown matrix format: {matrix_format}")
    parent = Path(parent)
    if not parent.is_dir():
        print(f"ERROR: {parent}: not a battery directory", file=sys.stderr)
        return 1
    foreign = foreign_entries(parent)
    if foreign:
        print(
            f"ERROR: {parent}: not a battery directory; foreign entries: {', '.join(foreign)}",
            file=sys.stderr,
        )
        return 1
    paths = _rep_dirs(parent)
    if not paths:
        print(f"ERROR: {parent}: no *-repN subdirectories", file=sys.stderr)
        return 1
    cells = []
    rows = []
    for path in paths:
        entry = _extract_entry(path)
        cells.append(entry)
        if "error" not in entry:
            rows.append(entry["row"])
    report_path = parent / "report.tsv"
    medians = [_report_row(row) for row in _median_rows(rows)]
    report_rows = []
    if report_path.is_file():
        try:
            report_rows = load_report(report_path)
        except ValueError as err:
            # A report written under an older column scheme cannot be
            # mapped onto this one; the rebuilt rows above are the extract's
            # record of the parent.
            print(f"warn: {err}; omitting report rows", file=sys.stderr)
    # The per-rep rows stay report-shaped in the document; their `samples`
    # helper key was only needed for the pooled medians above.
    cells = [
        {**entry, "row": _report_row(entry["row"])} if "row" in entry else entry for entry in cells
    ]
    document = {
        "battery": str(parent),
        "level": next((row.get("level") for row in rows if row.get("level")), None),
        "cells": cells,
        "medians": medians,
        "report": report_rows,
        "errors": sum(1 for entry in cells if "error" in entry),
        "environment": environment.load_environment(parent / environment.ENVIRONMENT_FILENAME),
    }
    payload = json.dumps(_json_safe(document), indent=2, allow_nan=False) + "\n"
    # The JSON stays the machine-readable record; the human views are
    # rendered beside it (`out`'s directory, else the battery parent).
    out_path = Path(out) if out is not None else None
    if out_path is not None:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(payload)
    elif not matrix_only:
        print(payload, end="")
    matrix_dir = out_path.parent if out_path is not None else parent
    human_text = None
    if "md" in formats:
        human_text = render_report(document)
        (matrix_dir / REPORT_FILE).write_text(human_text)
    if "tsv" in formats:
        tsv_text = render_matrix(medians)
        (matrix_dir / MATRIX_FILE).write_text(tsv_text)
        human_text = human_text or tsv_text
    if matrix_only:
        print(human_text, end="")
    if not rows:
        print(f"ERROR: {parent}: no rep subdirectory parsed", file=sys.stderr)
        return 2
    return 0


# --- baseline checking -------------------------------------------------------
@dataclass(frozen=True)
class Baseline:
    cell: str
    metric: str
    baseline: float
    recorded: str
    note: str


@dataclass(frozen=True)
class Failure:
    cell: str
    metric: str
    message: str


@dataclass(frozen=True)
class CheckResult:
    checked: int
    cells: int
    failures: list


def load_baselines(path: Path) -> list:
    """Read the long-format baseline file; comments and blanks are ignored."""
    lines = [
        line
        for line in Path(path).read_text().splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]
    if not lines:
        raise ValueError(f"{path}: empty baselines file")
    header = lines[0].split("\t")
    if header != BASELINE_COLUMNS:
        raise ValueError(f"{path}: unexpected baselines header")
    baselines = []
    for line in lines[1:]:
        parts = line.split("\t")
        if len(parts) != len(header):
            raise ValueError(f"{path}: malformed baselines row: {line}")
        cell, metric, baseline, recorded, note = parts
        try:
            value = float(baseline)
        except ValueError:
            raise ValueError(f"{path}: non-numeric baseline row: {line}")
        baselines.append(
            Baseline(
                cell=cell,
                metric=metric,
                baseline=value,
                recorded=recorded,
                note=note,
            )
        )
    return baselines


def _abs_floor(metric: str) -> float:
    """Absolute floor for the guardrail, in the metric's own unit."""
    if metric.endswith("_ms"):
        return 100.0
    if metric.endswith("_us"):
        return 100_000.0
    if metric.endswith("_s"):
        return 0.1
    if metric.endswith("_mb"):
        return 1.0
    return 0.0


def check_report(rows: list, baselines: list) -> CheckResult:
    """Compare report rows against baselines; never mutates the baselines.

    Guardrail, not a fitted band: a reported median passes when it is
    within 50% of the recorded value or within an absolute floor in its
    own unit (100 ms, 0.1 s, 100000 us, 1 MB). Anything else — including
    a missing or non-numeric value — fails.
    """
    median_rows = [row for row in rows if str(row.get("cell", "")).endswith(":median")]
    if median_rows:
        rows = median_rows
    by_cell = {row.get("cell"): row for row in rows}
    failures = []
    checked = 0
    referenced = set()
    for baseline in baselines:
        referenced.add(baseline.cell)
        row = by_cell.get(baseline.cell)
        band = max(_abs_floor(baseline.metric), 0.5 * abs(baseline.baseline))
        low = baseline.baseline - band
        high = baseline.baseline + band
        if row is None:
            failures.append(
                Failure(
                    baseline.cell,
                    baseline.metric,
                    f"FAIL cell={baseline.cell} metric={baseline.metric}: cell missing from report",
                )
            )
            continue
        actual_raw = row.get(baseline.metric, NA)
        if actual_raw == NA:
            failures.append(
                Failure(
                    baseline.cell,
                    baseline.metric,
                    f"FAIL cell={baseline.cell} metric={baseline.metric} actual=n/a "
                    f"baseline={baseline.baseline:g} band=[{low:g},{high:g}]",
                )
            )
            continue
        try:
            actual = float(actual_raw)
        except ValueError:
            failures.append(
                Failure(
                    baseline.cell,
                    baseline.metric,
                    f"FAIL cell={baseline.cell} metric={baseline.metric} actual={actual_raw} "
                    f"(not numeric) baseline={baseline.baseline:g} band=[{low:g},{high:g}]",
                )
            )
            continue
        if low <= actual <= high:
            checked += 1
            continue
        failures.append(
            Failure(
                baseline.cell,
                baseline.metric,
                f"FAIL cell={baseline.cell} metric={baseline.metric} actual={actual_raw} "
                f"baseline={baseline.baseline:g} band=[{low:g},{high:g}]",
            )
        )
    return CheckResult(checked=checked, cells=len(referenced), failures=failures)


# --- running -----------------------------------------------------------------
def _interrupt(signum, frame):
    raise KeyboardInterrupt


def _cell_env(
    base: dict,
    run_config: config.BatteryConfig,
    cell: Cell,
    index: int,
    cell_out: Path,
) -> dict:
    """The environment of one rep, derived from the pinned battery config."""
    env = dict(base)
    env.update(cell.env)
    env["BUILD_PROFILE"] = run_config.build_profile
    env["WORKLOAD_TIMEOUT_S"] = run_config.workload_timeout_s
    env["BATTERY_REPEATS"] = str(run_config.repeats)
    env.update(
        {
            "OUT": str(cell_out),
            "PORT": str(run_config.base_port + index),
            "IMAGE_REPO": str(repo_path(cell, run_config.sets_root)),
            "DATASET_ID": cell.repo,
        }
    )
    return env


def _reuse_row(level: str, cell: Cell, cell_out: Path) -> dict | None:
    """Return the report row of a completed rep, or None when it must run.

    run_cell writes `status-final.json` only for a terminal server state;
    a rep without that marker, or whose artifacts no longer parse
    (missing log, corrupt status), is incomplete and runs again.
    """
    if not (cell_out / "status-final.json").is_file():
        return None
    try:
        return build_row(level, cell.profile, cell_out, cell_id=cell.cell_id)
    except Exception, SystemExit:
        # Any parse failure marks the rep incomplete; a fresh run reports
        # its own row errors. KeyboardInterrupt still aborts the battery.
        return None


def run_battery(
    level: str,
    outdir: Path,
    *,
    sets_root: Path = DEFAULT_SETS_ROOT,
    base_port: int = DEFAULT_BASE_PORT,
    build_profile: str = "release",
    fresh: bool = False,
) -> int:
    """Run every cell sequentially, appending one row per cell.

    The report is rebuilt from scratch on every invocation. By default a
    completed rep is reused instead of re-run (`fresh=True` re-runs every
    rep), so an interrupted battery resumes where it stopped.

    The battery's knobs and budgets are validated once into a
    `BatteryConfig`; each rep's environment is derived from it.
    """
    outdir = Path(outdir).resolve()
    # The runner owns the parent: create it here, and refuse (or, with
    # --fresh, wipe) any foreign content left by a manual/loose cell.
    if fresh:
        if outdir.exists():
            shutil.rmtree(outdir)
        outdir.mkdir(parents=True, exist_ok=True)
    else:
        validate_parent(outdir, fresh=False)
        outdir.mkdir(parents=True, exist_ok=True)
    environ = dict(os.environ)
    # Cell level is fully parameterized by env: honor PORT/BUILD_PROFILE when
    # the caller did not override them explicitly.
    if build_profile == "release" and "BUILD_PROFILE" in environ:
        build_profile = config.validate_build_profile(environ, build_profile)
    if base_port == DEFAULT_BASE_PORT and "PORT" in environ:
        base_port = config.validate_port(environ["PORT"])
    run_config = config.BatteryConfig.from_env(
        environ,
        level=level,
        out=outdir,
        sets_root=sets_root,
        base_port=base_port,
        build_profile=build_profile,
        fresh=fresh,
    )
    n_repeats = run_config.repeats
    cells = battery_cells(level, environ)
    report = outdir / "report.tsv"
    report.unlink(missing_ok=True)
    orchestrator = run_cell

    # Host snapshot for this battery run. On resume the original start is
    # kept and only the resume/completion fields move. Telemetry must never
    # fail the battery, so every touch below is best-effort.
    env_path = outdir / environment.ENVIRONMENT_FILENAME
    try:
        env_record = environment.load_environment(env_path)
        if env_record is None:
            env_record = environment.collect_environment(level=level)
            environment.write_environment(env_path, env_record)
        else:
            env_record["resumed_utc"] = environment.collect_environment(level=level)["started_utc"]
            environment.write_environment(env_path, env_record)
    except Exception as err:  # noqa: BLE001 - telemetry must not fail the battery
        print(f"[battery {level}] environment snapshot skipped: {err}", file=sys.stderr)

    try:
        for index, cell in enumerate(cells):
            repeat_rows = []
            for rep in range(1, n_repeats + 1):
                cell_out = outdir / f"{cell.outdir_suffix}-rep{rep}"
                # Default resume: a completed rep is reused; --fresh re-runs it.
                row = None if fresh else _reuse_row(level, cell, cell_out)
                if row is not None:
                    print(
                        f"[battery {level}] {cell.cell_id}: rep={rep}/{n_repeats} reused {cell_out}",
                        flush=True,
                    )
                    append_row(report, row)
                    repeat_rows.append(row)
                    continue
                env = _cell_env(environ, run_config, cell, index, cell_out)
                print(
                    f"[battery {level}] {cell.cell_id}: rep={rep}/{n_repeats} port={env['PORT']} out={cell_out}",
                    flush=True,
                )
                cfg = orchestrator.config_from_env(env)
                rc = orchestrator.run_cell(cfg, env)
                try:
                    row = build_row(level, cell.profile, cell_out, cell_id=cell.cell_id)
                except (OSError, ValueError, SystemExit) as err:
                    print(
                        f"[battery {level}] {cell.cell_id}: no result row: {err}", file=sys.stderr
                    )
                else:
                    append_row(report, row)
                    repeat_rows.append(row)
                    print(f"[battery {level}] {cell.cell_id}: row appended (rc={rc})", flush=True)
                if rc != 0:
                    print(f"[battery {level}] {cell.cell_id}: cell failed rc={rc}", file=sys.stderr)
                    return rc
            if n_repeats > 1 and repeat_rows:
                median_row = _build_median_row(repeat_rows)
                append_row(report, median_row)
                print(f"[battery {level}] {cell.cell_id}: median row appended", flush=True)
        print(f"[battery {level}] report: {report}", flush=True)
        return 0
    finally:
        # Stamp completion (started-/loadavg-at-start stay as recorded).
        try:
            done = environment.load_environment(env_path)
            if done is None:
                done = environment.collect_environment(level=level)
            environment.write_environment(env_path, environment.finish_environment(done))
        except Exception as err:  # noqa: BLE001 - telemetry must not fail the battery
            print(f"[battery {level}] environment finish skipped: {err}", file=sys.stderr)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    run = subparsers.add_parser("run", help="run a battery and write its report")
    run.add_argument("level", choices=sorted(_KNOWN_LEVELS))
    run.add_argument("--out", help="battery directory (default: $OUT)")
    run.add_argument(
        "--sets-root",
        default=os.environ.get("BENCH_SETS_ROOT", str(DEFAULT_SETS_ROOT)),
        help="image repo root (default: $BENCH_SETS_ROOT or ~/picasu-bench)",
    )
    run.add_argument(
        "--port",
        type=int,
        default=int(os.environ.get("BENCH_PORT", DEFAULT_BASE_PORT)),
        help="first cell port; each cell increments (default: 5795)",
    )
    run.add_argument("--build", default="release", choices=("release", "debug"))
    run.add_argument(
        "--fresh",
        action="store_true",
        help="re-run every rep instead of resuming completed ones (default: resume)",
    )

    check = subparsers.add_parser("check", help="compare a report against baselines")
    check.add_argument("report", help="battery report TSV")
    check.add_argument(
        "--baselines",
        default=str(SCRIPT_DIR / "baselines.tsv"),
        help="baseline TSV (default: utils/bench/baselines.tsv)",
    )

    listing = subparsers.add_parser("list", help="print a battery's cell ids")
    listing.add_argument("level", choices=sorted(_KNOWN_LEVELS))

    extract = subparsers.add_parser(
        "extract", help="dump battery parents as JSON documents, or record baselines"
    )
    extract.add_argument(
        "parents",
        nargs="+",
        help="battery directories, or a run root containing them",
    )
    extract.add_argument("--out", help="write the JSON document to this file (default: stdout)")
    extract.add_argument(
        "--format",
        choices=("md", "tsv", "both"),
        default="both",
        help="human view artifacts to write next to the JSON (default: both)",
    )
    extract.add_argument(
        "--matrix-only",
        action="store_true",
        help="print the human view on stdout instead of the JSON document "
        "(the Markdown report, or the TSV with --format tsv)",
    )
    extract.add_argument(
        "--baselines-tsv",
        nargs="?",
        const="-",
        default=None,
        metavar="PATH",
        help="merge every parent's medians into this baselines.tsv long format "
        "(no PATH prints it); combine with --render-md for the full doc",
    )
    extract.add_argument(
        "--baselines",
        default=None,
        metavar="PATH",
        help="merge source for --baselines-tsv (default: the output path when it "
        "exists, else utils/bench/baselines.tsv)",
    )
    extract.add_argument(
        "--render-md",
        default=None,
        metavar="PATH",
        help="render the full baselines.md from the merged baselines plus every "
        "parent's report.tsv (needs --baselines-tsv)",
    )

    args = parser.parse_args(argv)

    if args.command == "run":
        raw_out = args.out or os.environ.get("OUT", "")
        if raw_out == "":
            raw_out = f"/tmp/picasu-battery-{args.level}-{datetime.now():%Y%m%d-%H%M%S}"
        # SIGTERM unwinds through run_cell's finally so the launcher and its
        # server are stopped instead of orphaned between cells.
        signal.signal(signal.SIGTERM, _interrupt)
        try:
            return run_battery(
                args.level,
                Path(raw_out),
                sets_root=Path(args.sets_root),
                base_port=args.port,
                build_profile=args.build,
                fresh=args.fresh,
            )
        except (ConfigError, ValueError) as err:
            print(f"ERROR: {err}", file=sys.stderr)
            return 1
        except KeyboardInterrupt:
            print("[battery] interrupted", file=sys.stderr)
            return 130

    if args.command == "check":
        report_path = Path(args.report)
        if report_path.is_dir():
            parent = report_path
            report_path = parent / "report.tsv"
        else:
            parent = report_path.parent
        try:
            validate_parent(parent, fresh=False)
            rows = load_report(report_path)
            baselines = load_baselines(Path(args.baselines or str(SCRIPT_DIR / "baselines.tsv")))
        except (OSError, ValueError, ConfigError) as err:
            print(f"ERROR: {err}", file=sys.stderr)
            return 1
        result = check_report(rows, baselines)
        if not baselines:
            print("note: no baseline rows recorded; nothing to check")
        for failure in result.failures:
            print(failure.message)
        baselined_metrics = {baseline.metric for baseline in baselines}
        unbaselined = [
            column
            for column in (METRICS_COLUMNS + SCROLL_COLUMNS + RANGE_COLUMNS)
            if column not in baselined_metrics
        ]
        if unbaselined:
            print("informational (no baseline recorded): " + ", ".join(unbaselined))
        print(
            f"{len(result.failures)} failure(s); {result.checked} metric(s) within guardrail "
            f"across {result.cells} cell(s)"
        )
        return 1 if result.failures else 0

    if args.command == "extract":
        try:
            if args.baselines_tsv is not None or args.render_md is not None:
                if args.render_md is not None and args.baselines_tsv is None:
                    print("ERROR: --render-md needs --baselines-tsv", file=sys.stderr)
                    return 2
                if args.out is not None or args.matrix_only:
                    print(
                        "ERROR: --out/--matrix-only take exactly one battery parent",
                        file=sys.stderr,
                    )
                    return 2
                return record_baselines(
                    args.parents,
                    args.baselines_tsv,
                    merge_path=args.baselines,
                    md_path=args.render_md,
                )
            if len(args.parents) != 1:
                print(
                    "ERROR: pass exactly one battery parent "
                    "(or --baselines-tsv/--render-md for several)",
                    file=sys.stderr,
                )
                return 2
            return extract_battery(
                Path(args.parents[0]),
                Path(args.out) if args.out else None,
                matrix_only=args.matrix_only,
                matrix_format=args.format,
            )
        except (OSError, ValueError, ConfigError) as err:
            # A corrupt report.tsv surfaces the same way `check` reports it.
            print(f"ERROR: {err}", file=sys.stderr)
            return 1

    for cell in battery_cells(args.level):
        print(cell.cell_id)
    return 0


if __name__ == "__main__":
    sys.exit(main())
