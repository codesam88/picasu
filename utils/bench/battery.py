#!/usr/bin/env python3
"""Battery runner and baseline checker.

A battery is an ordered, pinned set of benchmark cells for one scale. Each
cell runs through `run-cell.py` unchanged; after the cell finishes the
extractor's parsed values become one row in a TSV run report. The report is
compared against `baselines.tsv`, where every accepted metric carries a
tolerance band.

Usage:
    utils/bench/battery.py run gate --out /tmp/picasu-battery-gate
    utils/bench/battery.py check /tmp/picasu-battery-gate/report.tsv
    utils/bench/battery.py list gate
    utils/bench/battery.py extract /tmp/picasu-battery-gate [--out run.json]

`run` resumes by default: the report is rebuilt from scratch on every
invocation and each rep whose `status-final.json` parses is reused
instead of re-run, so an interrupted battery continues where it stopped.
`--fresh` re-runs every rep.

`extract` dumps one battery parent (its `*-repN` subdirs plus
`report.tsv`) as a single JSON document: per-rep report rows, manifest
identity, stages, terminal state, pooled medians and the report rows.

Environment:
    OUT                  battery directory for `run` when --out is absent
    BENCH_SETS_ROOT      image repo root (default /tmp/sets)
    BENCH_PORT           first cell port (default 5795); cells increment
    IMAGE_PATH           passed through to each cell by run-cell.py
    WORKLOAD_TIMEOUT_S   per-cell workload budget handed to run-cell
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

Baselines are deliberate: `check` never updates the file. A cell outside
its band fails until the move is explained and the row is re-recorded.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import os
import re
import shutil
import signal
import statistics
import sys
from dataclasses import dataclass
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))
import config  # noqa: E402
from config import DEFAULT_SETS_ROOT, ConfigError  # noqa: E402

DEFAULT_BASE_PORT = config.BATTERY_BASE_PORT
NA = "n/a"

REPORT_COLUMNS = [
    "cell",
    "profile",
    "level",
    "scale",
    "build",
    "commit",
    "scan_wall_s",
    "scan_walls_s",
    "scan_fetch_p50_ms",
    "fetch_worst_ms",
    "scan_thumb_p50_ms",
    "thumb_worst_ms",
    "scan_edit_p50_ms",
    "edit_worst_ms",
    "idle_thumb_ms",
    "idle_fetch_ms",
    "idle_edit_ms",
    "upload_submit_p50_ms",
    "upload_visible_p50_ms",
    "tree_refresh_build_p50_us",
    "tree_refresh_build_max_us",
    "filter_snapshot_clone_p50_us",
    "filter_snapshot_clone_max_us",
]

BASELINE_COLUMNS = ["cell", "metric", "baseline", "rel_tol", "abs_tol", "recorded", "note"]
BASELINE_HEADER = "\t".join(BASELINE_COLUMNS)

_UPLOAD_P50 = re.compile(r"upload submit_p50=(\d+) visible_p50=(\d+)")

_MODULE_CACHE: dict = {}


def _load_module(name: str, path: Path):
    if name not in _MODULE_CACHE:
        spec = importlib.util.spec_from_file_location(name, path)
        module = importlib.util.module_from_spec(spec)
        sys.modules[name] = module
        spec.loader.exec_module(module)
        _MODULE_CACHE[name] = module
    return _MODULE_CACHE[name]


def _extractor():
    return _load_module("extract_bench", SCRIPT_DIR / "extract-bench.py")


def _run_cell():
    return _load_module("run_cell", SCRIPT_DIR / "run-cell.py")


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

    @property
    def cell_id(self) -> str:
        return f"{self.level}-{self.profile}"


def _cell(level: str, profile: str, images: int, repo: str) -> Cell:
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
        outdir_suffix=f"{level}-{profile}",
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
}


_KNOWN_LEVELS = ("cell", "gate", "scale")


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
    return tuple(
        _cell(level, profile, spec["images"], spec["repo"]) for profile in spec["profiles"]
    )


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


def build_row(level: str, profile: str, outdir: Path) -> dict:
    """Flatten one cell outdir into the run-report row schema."""
    outdir = Path(outdir)
    extractor = _extractor()
    parsed = extractor.parse_log(outdir)
    status = extractor.parse_status(outdir)
    # Empty storm-key list: request latency and S2 drain columns are what
    # the battery stores; profile-specific storm aggregates stay in the
    # raw outdir and extractor table.
    cell = extractor._cell(parsed, status, [])
    manifest = _read_manifest(outdir)
    try:
        log_text = (outdir / "harness-a.log").read_text()
    except OSError:
        log_text = ""
    upload_submit, upload_visible = _parse_upload_p50(log_text)

    walls = parsed.get("scan_runs_ms", [])
    return {
        "cell": f"{level}-{profile}",
        "profile": profile,
        "level": level,
        "scale": _fmt(parsed.get("images")),
        "build": _fmt(manifest.get("build_profile")),
        "commit": _fmt(manifest.get("git")),
        "scan_wall_s": _fmt(parsed.get("scan_wall_s")),
        "scan_walls_s": ",".join(str(wall) for wall in walls) if walls else NA,
        "scan_fetch_p50_ms": _fmt(cell.get("scan_fetch_ms")),
        "fetch_worst_ms": _fmt(cell.get("fetch_worst_ms")),
        "scan_thumb_p50_ms": _fmt(cell.get("scan_thumb_ms")),
        "thumb_worst_ms": _fmt(cell.get("thumb_worst_ms")),
        "scan_edit_p50_ms": _fmt(cell.get("scan_edit_ms")),
        "edit_worst_ms": _fmt(cell.get("edit_worst_ms")),
        "idle_thumb_ms": _fmt(cell.get("idle_thumb_ms")),
        "idle_fetch_ms": _fmt(cell.get("idle_fetch_ms")),
        "idle_edit_ms": _fmt(cell.get("idle_edit_ms")),
        "upload_submit_p50_ms": upload_submit,
        "upload_visible_p50_ms": upload_visible,
        "tree_refresh_build_p50_us": _fmt(cell.get("tree_refresh_build_p50_us")),
        "tree_refresh_build_max_us": _fmt(cell.get("tree_refresh_build_max_us")),
        "filter_snapshot_clone_p50_us": _fmt(cell.get("filter_snapshot_clone_p50_us")),
        "filter_snapshot_clone_max_us": _fmt(cell.get("filter_snapshot_clone_max_us")),
    }


_NON_NUMERIC_COLUMNS = {
    "cell",
    "profile",
    "level",
    "scale",
    "build",
    "commit",
}


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
        values = []
        for row in rows:
            values.extend(_numeric_values(row.get(column, NA)))
        # Round out binary-float noise from averaging two odd-count
        # neighbours (ms/1000 seconds land as 6.657500000000001); every
        # report column resolves well above 1e-6.
        result[column] = _fmt(round(statistics.median(values), 6)) if values else NA
    result["cell"] = median_cell
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
_BATTERY_PARENT_ALLOWED_FILES = {"report.tsv", "report.json", "orchestrator.log"}


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
    """Parsed `stages.json` content; an absent or corrupt file yields []."""
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
    level, _, profile = cell.partition("-")
    if not level or not profile:
        return {
            "dir": path.name,
            "error": "cannot derive level and profile from directory name",
        }
    try:
        row = build_row(level, profile, path)
    except (Exception, SystemExit) as err:
        # Broken reps become error entries so one bad dir cannot abort the
        # walk; KeyboardInterrupt still aborts the extract.
        if isinstance(err, SystemExit):
            message = f"extractor exited with code {err.code}"
        else:
            message = str(err) or type(err).__name__
        return {"dir": path.name, "error": message}
    return {
        "dir": path.name,
        "row": row,
        "manifest": _manifest_identity(path),
        "stages": _read_stages(path),
        "terminal": _terminal_state(path),
    }


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


def extract_battery(parent: Path, out: Path | None = None) -> int:
    """Dump one battery parent as a single JSON document.

    Walks the `<level>-<profile>-repN` subdirs in sorted order and emits
    one entry per subdir (row, manifest identity, stages, terminal state;
    a rep whose artifacts no longer parse becomes an `{"dir", "error"}`
    entry), the pooled median rows, the parent's `report.tsv` rows and an
    `errors` count. Exit 0 when at least one rep parsed, 1 when the parent
    holds no rep subdirs, 2 when every rep failed to parse.
    """
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
    document = {
        "battery": str(parent),
        "level": next((row.get("level") for row in rows if row.get("level")), None),
        "cells": cells,
        "medians": _median_rows(rows),
        "report": load_report(report_path) if report_path.is_file() else [],
        "errors": sum(1 for entry in cells if "error" in entry),
    }
    payload = json.dumps(_json_safe(document), indent=2, allow_nan=False) + "\n"
    if out is None:
        print(payload, end="")
    else:
        out = Path(out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(payload)
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
    rel_tol: float
    abs_tol: float
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
        cell, metric, baseline, rel_tol, abs_tol, recorded, note = parts
        try:
            value = float(baseline)
            rel = float(rel_tol) if rel_tol != "" else 0.0
            absolute = float(abs_tol) if abs_tol != "" else 0.0
        except ValueError:
            raise ValueError(f"{path}: non-numeric baseline row: {line}")
        baselines.append(
            Baseline(
                cell=cell,
                metric=metric,
                baseline=value,
                rel_tol=rel,
                abs_tol=absolute,
                recorded=recorded,
                note=note,
            )
        )
    return baselines


def check_report(rows: list, baselines: list) -> CheckResult:
    """Compare report rows against baselines; never mutates the baselines."""
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
        band = max(baseline.abs_tol, abs(baseline.baseline) * baseline.rel_tol)
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

    run-cell writes `status-final.json` only for a terminal server state;
    a rep without that marker, or whose artifacts no longer parse
    (missing log, corrupt status), is incomplete and runs again.
    """
    if not (cell_out / "status-final.json").is_file():
        return None
    try:
        return build_row(level, cell.profile, cell_out)
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
    orchestrator = _run_cell()

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
                row = build_row(level, cell.profile, cell_out)
            except (OSError, ValueError, SystemExit) as err:
                print(f"[battery {level}] {cell.cell_id}: no result row: {err}", file=sys.stderr)
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


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    run = subparsers.add_parser("run", help="run a battery and write its report")
    run.add_argument("level", choices=sorted(_KNOWN_LEVELS))
    run.add_argument("--out", help="battery directory (default: $OUT)")
    run.add_argument(
        "--sets-root",
        default=os.environ.get("BENCH_SETS_ROOT", str(DEFAULT_SETS_ROOT)),
        help="image repo root (default: $BENCH_SETS_ROOT or /tmp/sets)",
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
        "extract", help="dump one battery parent as a single JSON document"
    )
    extract.add_argument("parent", help="battery directory holding *-repN subdirs")
    extract.add_argument("--out", help="write the JSON document to this file (default: stdout)")

    args = parser.parse_args(argv)

    if args.command == "run":
        raw_out = args.out or os.environ.get("OUT", "")
        if raw_out == "":
            print("ERROR: --out or OUT is required", file=sys.stderr)
            return 1
        # SIGTERM unwinds through run-cell's finally so the launcher and its
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
            baselines = load_baselines(Path(args.baselines))
        except (OSError, ValueError, ConfigError) as err:
            print(f"ERROR: {err}", file=sys.stderr)
            return 1
        result = check_report(rows, baselines)
        if not baselines:
            print("note: no baseline rows recorded; nothing to check")
        for failure in result.failures:
            print(failure.message)
        print(
            f"{len(result.failures)} failure(s); {result.checked} metric(s) in band "
            f"across {result.cells} cell(s)"
        )
        return 1 if result.failures else 0

    if args.command == "extract":
        try:
            return extract_battery(Path(args.parent), Path(args.out) if args.out else None)
        except (OSError, ValueError, ConfigError) as err:
            # A corrupt report.tsv surfaces the same way `check` reports it.
            print(f"ERROR: {err}", file=sys.stderr)
            return 1

    for cell in battery_cells(args.level):
        print(cell.cell_id)
    return 0


if __name__ == "__main__":
    sys.exit(main())
