#!/usr/bin/env python3
"""Battery runner and baseline recorder.

A battery is the ordered profile matrix for one image count. Each cell runs
through `run_cell.py`; after it finishes the extractor's samples become rows
in a long TSV run report. Accepted values live in `baselines.tsv`;
`compare_reports.py` reviews a report against them without gating.

Usage:
    utils/bench/battery.py run --images 100 --out /tmp/picasu-battery-100
    utils/bench/battery.py list --images 100
    utils/bench/battery.py extract /tmp/picasu-battery-100 [--out run.json] \\
        [--baselines-tsv PATH] [--render-md PATH]

`run` resumes by default: the report is rebuilt from scratch on every
invocation and each rep whose `status-final.json` parses is reused instead
of re-run, so an interrupted battery continues where it stopped. `--fresh`
re-runs every rep.

`extract` dumps one battery parent (its `*-repN` subdirs plus `report.tsv`)
as a single JSON document. The JSON goes to stdout or, with `--out`, to that
file; a plain extract writes nothing else. `--baselines-tsv` merges every
parent's rows into one long-format baselines file; `--render-md` renders the
full `baselines.md` from the merged values and their cell metadata.

Environment:
    OUT                  battery directory for `run` when --out is absent
    BENCH_SETS_ROOT      image repo root (default ~/picasu-bench)
    BENCH_PORT           first cell port (default 5795); cells increment
    IMAGE_PATH           passed through to each cell by run_cell.py
    WORKLOAD_TIMEOUT_S   per-cell workload budget (default 1800 s up to
                         200 images, 7200 s above); an exported value wins
    BATTERY_REPEATS      cap on scan repetitions per cell (default 5 up to
                         200 images, 3 above); an exported value wins

Batteries pin every knob that changes a measurement (`SCAN_REPEATS`,
hyperfine `REPEATS`/`WARMUP`, storm client/op counts and margin) plus the
build profile, image set and CPU count. Cells run sequentially: backend
tests and bench cells share tempdir state, so parallel cells would
contaminate each other.

Baselines are deliberate: only `extract --baselines-tsv` updates the file,
and compare reports are a review tool, not a gate.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import shutil
import signal
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
import metrics  # noqa: E402
import run_cell  # noqa: E402
from config import DEFAULT_SETS_ROOT, ConfigError  # noqa: E402

DEFAULT_BASE_PORT = config.BATTERY_BASE_PORT
NA = "n/a"

REPORT_COLUMNS = tuple(metrics.REPORT_COLUMNS)
BASELINE_COLUMNS = tuple(metrics.BASELINE_COLUMNS)
BASELINE_HEADER = "\t".join(BASELINE_COLUMNS)
BASELINE_INDEX = {name: index for index, name in enumerate(BASELINE_COLUMNS)}


# --- battery definition ------------------------------------------------------
@dataclass(frozen=True)
class Cell:
    profile: str
    images: int
    repo: str
    ncpu: int
    env: dict
    outdir_suffix: str
    name: str = ""
    source: str = "battery"

    @property
    def cell_id(self) -> str:
        return self.name or f"{self.profile}-{self.images}"


def _cell(
    profile: str,
    images: int,
    repo: str,
    *,
    cell_name: str | None = None,
    outdir_suffix: str | None = None,
    source: str = "battery",
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
        derived = config.storm_profile_clients(profile, 8)
        storm_clients = derived if derived is not None else config.DEFAULT_STORM_CLIENTS
        env.update(
            {
                "STORM_CLIENTS": str(storm_clients),
                "STORM_OPS": "10",
            }
        )
    if profile == "upload":
        env["UPLOAD_FILES"] = str(config.DEFAULT_UPLOAD_FILES)
        # Uploaded files come from a distinct archive: the cell's own repo is
        # fully staged, so re-uploading it would ingest already-indexed images.
        env["UPLOAD_REPO"] = "set-upload"
    return Cell(
        profile=profile,
        images=images,
        repo=repo,
        ncpu=8,
        env=env,
        outdir_suffix=outdir_suffix if outdir_suffix is not None else f"{profile}-{images}",
        name=cell_name if cell_name is not None else "",
        source=source,
    )


_SCENARIO_PROFILES = (
    "control",
    *config.CONCURRENT_STORM_PROFILES,
    *(profile for profile in config.IDLE_STORM_PROFILES if "-read-" in profile),
    "upload",
)

CELL_ORDER = {}


def storm_profile_access(profile: str) -> str:
    """Access a storm profile drives; the profile names it explicitly."""
    access = config.storm_profile_access(profile)
    return access if access is not None else "read"


def _cell_from_env(environ: dict) -> Cell:
    """Build the single cell for the debug `cell` battery from the environment."""
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

    if "STORM_CLIENTS" in environ:
        storm_clients = config.integer(
            environ, "STORM_CLIENTS", config.DEFAULT_STORM_CLIENTS, minimum=1
        )
    else:
        derived = config.storm_profile_clients(profile, 8)
        storm_clients = derived if derived is not None else config.DEFAULT_STORM_CLIENTS
    storm_ops = (
        config.integer(environ, "STORM_OPS", config.DEFAULT_STORM_OPS, minimum=1)
        if "STORM_OPS" in environ
        else config.DEFAULT_STORM_OPS
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
            }
        )
    if profile == "upload":
        upload_files = config.integer(
            environ, "UPLOAD_FILES", config.DEFAULT_UPLOAD_FILES, minimum=1
        )
        env["UPLOAD_FILES"] = str(upload_files)
        if "UPLOAD_REPO" in environ:
            env["UPLOAD_REPO"] = environ["UPLOAD_REPO"]

    return Cell(
        profile=profile,
        images=images,
        repo=repo,
        ncpu=ncpu,
        env=env,
        outdir_suffix=f"{profile}-{images}",
        source="debug",
    )


def battery_cells(images: int) -> tuple:
    """The full scenario matrix at one image count."""
    images = int(images)
    repo = f"set-{images}"
    return tuple(_cell(profile, images, repo) for profile in _SCENARIO_PROFILES)


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


def build_row(
    profile: str, outdir: Path, *, cell_id: str | None = None, source: str = "battery"
) -> dict:
    """One per-rep record: identity, validity, samples, clusters."""
    outdir = Path(outdir)
    extractor = extract_bench
    parsed = extractor.parse_log(outdir, profile=profile)
    manifest = _read_manifest(outdir)
    identity = {
        "source": source,
        "profile": profile,
        "images": parsed.get("images"),
        "upload_files": (parsed.get("raw_cluster_counts") or {}).get("upload_submit"),
        "build": _fmt(manifest.get("build_profile")),
        "commit": _fmt(manifest.get("git")),
        "scan_runs": parsed.get("scan_runs"),
    }
    if cell_id is not None:
        identity["cell"] = cell_id
    if parsed.get("identity"):
        identity.update({k: v for k, v in parsed["identity"].items() if v is not None})
    identity["source"] = source
    if cell_id is not None:
        identity["cell"] = cell_id
    validity = parsed.get("validity") or {}
    record = {
        "cell": cell_id if cell_id is not None else f"{profile}-{parsed.get('images')}",
        "identity": identity,
        "validity": validity,
        "samples": parsed.get("samples") or {},
        "clusters": parsed.get("clusters") or {},
    }
    return record


def _row_line(row: dict) -> str:
    return "\t".join(
        str(row.get(column, NA)).replace("\t", " ").replace("\n", " ") for column in REPORT_COLUMNS
    )


def _cell_line(identity: dict, validity: dict) -> str:
    payload = {"identity": identity, "validity": validity}
    return "# cell: " + json.dumps(payload, sort_keys=True)


def write_report(path: Path, cells: list) -> None:
    """Write a report from cell blocks: `(identity, validity, rows)` each."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    lines = ["\t".join(REPORT_COLUMNS)]
    for identity, validity, rows in cells:
        lines.append(_cell_line(identity, validity))
        lines.extend(_row_line(row) for row in rows)
    path.write_text("\n".join(lines) + "\n")


def append_cell_rows(path: Path, identity: dict, validity: dict, rows: list) -> None:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        path.write_text("\t".join(REPORT_COLUMNS) + "\n")
    with path.open("a", encoding="utf-8") as handle:
        handle.write(_cell_line(identity, validity) + "\n")
        for row in rows:
            handle.write(_row_line(row) + "\n")


def load_report(path: Path) -> list:
    """Read a long report; returns one dict per cell block.

    Each block carries `identity`, `validity` and its `rows`. A report
    without metadata (an older run) yields one block with empty metadata.
    """
    lines = [line for line in Path(path).read_text().splitlines() if line.strip()]
    if not lines:
        raise ValueError(f"{path}: empty report")
    header_at = next((index for index, line in enumerate(lines) if line.startswith("cell\t")), None)
    if header_at is None or lines[header_at].split("\t") != list(REPORT_COLUMNS):
        raise ValueError(f"{path}: unexpected report header")
    cells: list = []
    current = {"identity": {}, "validity": {}, "rows": []}
    for line in lines[header_at + 1 :]:
        if line.startswith("# cell: "):
            if current["rows"] or current["identity"] or current["validity"]:
                cells.append(current)
            try:
                payload = json.loads(line[len("# cell: ") :])
            except json.JSONDecodeError as err:
                raise ValueError(f"{path}: bad cell metadata: {err}") from err
            current = {
                "identity": payload.get("identity") or {},
                "validity": payload.get("validity") or {},
                "rows": [],
            }
            continue
        parts = line.split("\t")
        if len(parts) != len(REPORT_COLUMNS):
            raise ValueError(f"{path}: malformed row with {len(parts)} columns: {line}")
        current["rows"].append(dict(zip(REPORT_COLUMNS, parts, strict=True)))
    if current["rows"] or current["identity"] or current["validity"]:
        cells.append(current)
    return cells


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
# `report.md`, `matrix.tsv` and `confidence.tsv` are no longer produced;
# they stay accepted so old run directories still validate, and are never
# modified by the extract. `environment.json` is the host snapshot taken
# when the battery started (and refreshed when it finished).
_BATTERY_PARENT_ALLOWED_FILES = {
    "report.tsv",
    "report.json",
    "orchestrator.log",
    "report.md",
    "matrix.tsv",
    "confidence.tsv",
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
    """Sorted `<profile>-<images>-repN` subdirectories of one battery parent."""
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
    profile, separator, images = cell.rpartition("-")
    if not separator or not images.isdigit() or profile not in config.WORKLOADS:
        return {
            "dir": path.name,
            "error": "cannot derive profile and images from directory name",
        }
    try:
        row = build_row(profile, path, cell_id=cell, source="battery")
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
    # Keep the stage rows on `row` as well: the diagnostic CI builder reads
    # their per-scan p50s before `_report_row` strips helper keys.
    stages = row.get("stages") or _read_stages(path)
    return {
        "dir": path.name,
        "row": row,
        "manifest": _manifest_identity(path),
        "stages": stages,
        "terminal": _terminal_state(path),
    }


def _pooled_validity(records: list) -> dict:
    """Cell validity across reps: round sums, worst coverage, unioned reasons."""
    validity: dict = {}
    for field in ("probe_rounds_total", "probe_rounds_valid", "phase_invalid"):
        values = []
        for record in records:
            try:
                values.append(int((record.get("validity") or {}).get(field)))
            except TypeError, ValueError:
                continue
        if values:
            validity[field] = sum(values)
    coverages = []
    for record in records:
        try:
            coverages.append(float((record.get("validity") or {}).get("probe_coverage_pct")))
        except TypeError, ValueError:
            continue
    validity["probe_coverage_pct"] = min(coverages) if coverages else NA
    warmups = [(record.get("validity") or {}).get("warmup_valid") for record in records]
    recorded = [value for value in warmups if value not in (NA, None)]
    if not recorded:
        validity["warmup_valid"] = NA
    elif any(value == "false" for value in recorded):
        validity["warmup_valid"] = "false"
    else:
        validity["warmup_valid"] = "true"
    reasons = []
    for record in records:
        raw = (record.get("validity") or {}).get("phase_invalid_reasons")
        if raw in (NA, None):
            continue
        for reason in str(raw).split("; "):
            if reason and reason not in reasons:
                reasons.append(reason)
    validity["phase_invalid_reasons"] = "; ".join(reasons) if reasons else NA
    for field in ("storm_coverage_pct", "ops_done", "ops_planned"):
        for record in records:
            value = (record.get("validity") or {}).get(field)
            if value not in (NA, None):
                validity[field] = value
                break
    failures = [
        (record.get("validity") or {}).get("failed")
        for record in records
        if (record.get("validity") or {}).get("failed") not in (NA, None)
    ]
    if failures:
        validity["failed"] = sum(int(value) for value in failures)
    validity["reps"] = len(records)
    return validity


def _pooled_report_rows(records: list):
    """Pool per-rep samples into long report rows; returns (rows, identity, validity).

    Every statistic of a metric comes from the same pooled multiset, and the
    p50 interval is a bootstrap over that same multiset. Nothing here reads
    a per-rep summary.
    """
    if not records:
        return [], {}, {}
    pooled_samples: dict = {}
    pooled_clusters: dict = {}
    for record in records:
        for key, values in (record.get("samples") or {}).items():
            pooled_samples.setdefault(key, []).extend(values)
        for key, count in (record.get("clusters") or {}).items():
            pooled_clusters[key] = pooled_clusters.get(key, 0) + int(count or 0)
    cell = records[0].get("cell", "")
    identity = dict(records[0].get("identity") or {})
    identity["cell"] = cell
    validity = _pooled_validity(records)
    rows = []
    for key, values in pooled_samples.items():
        family, background, subject = metrics.split_key(key)
        summary = metrics.summarize(values, family)
        for stat in metrics.statistics_for(family):
            low = high = NA
            if stat == "p50":
                low, high = metrics.bootstrap_interval(values)
                low = NA if low is None else f"{float(low):.1f}"
                high = NA if high is None else f"{float(high):.1f}"
            if stat == metrics.COUNT_STAT:
                value = str(int(round(float(summary[stat]))))
            else:
                value = f"{float(summary[stat]):.1f}"
            rows.append(
                {
                    "cell": cell,
                    "family": family,
                    "background": background,
                    "subject": subject,
                    "stat": stat,
                    "value": value,
                    "unit": metrics.UNITS[family],
                    "n_samples": len(values),
                    "n_clusters": pooled_clusters.get(key, 0),
                    "ci_low": low,
                    "ci_high": high,
                }
            )
    rows.sort(key=_row_sort_key)
    return rows, identity, validity


def _row_sort_key(row: dict):
    return (
        str(row.get("family", "")),
        str(row.get("background", "")),
        str(row.get("subject", "")),
        metrics.STATS.index(row["stat"]) if row.get("stat") in metrics.STATS else 0,
    )


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
    "# Accepted benchmark values for the recorded image counts.",
    "#",
    "# Long format, one row per cell and metric:",
    "#   cell family background subject stat value unit",
    "#   n_samples n_clusters ci_low ci_high recorded note",
    "#",
    "# Values and CI bounds use one decimal; `n/a` marks metrics without an",
    "# interval. `compare_reports.py` reviews reports against these values;",
    "# it is a review tool, not a gate.",
]


def _baseline_key(parts) -> tuple:
    """Identity of a baseline/report row: cell plus metric dimensions."""
    return tuple(str(part) for part in parts[:5])


def load_baselines(path: Path) -> list:
    """Read baselines.tsv rows keyed by cell and metric dimensions."""
    rows = []
    if not Path(path).is_file():
        return rows
    header = "	".join(BASELINE_COLUMNS)
    for line in Path(path).read_text().splitlines():
        if not line or line.startswith("#") or line == header:
            continue
        parts = line.split("	")
        if len(parts) != len(BASELINE_COLUMNS):
            raise ValueError(f"{path}: malformed baselines row: {line}")
        rows.append(dict(zip(BASELINE_COLUMNS, parts, strict=True)))
    return rows


def _baseline_value(value) -> str:
    """Value in the file's one-decimal style (8.26 -> 8.3)."""
    if value in (None, "", NA):
        return NA
    try:
        return f"{round(float(value), 1):.1f}"
    except TypeError, ValueError:
        return str(value)


def _baseline_line(row: dict) -> str:
    return "\t".join(
        str(row.get(column, NA)).replace("\t", " ").replace("\n", " ")
        for column in BASELINE_COLUMNS
    )


def merge_baselines(
    rows: list,
    existing: list,
    *,
    run_date: str,
    branch: str,
    commit: str,
) -> tuple[list, list]:
    """Merge accepted report rows into the baselines row set.

    Rows for a cell present in this run replace the stored value, interval
    and provenance; rows for cells absent from this run carry over unchanged.
    Notes survive re-recording.
    """
    notes = {
        _baseline_key([row[column] for column in REPORT_COLUMNS]): row["note"] for row in existing
    }
    covered = {str(row["cell"]) for row in rows}
    merged = []
    new_metrics = []
    recorded = f"{run_date} {branch} {commit}".strip()
    for row in rows:
        parts = [row[column] for column in REPORT_COLUMNS]
        key = _baseline_key(parts)
        note = notes.get(key)
        if note is None:
            note = f"new metric recorded {run_date}"
            new_metrics.append((row["cell"], key[1:]))
        if row["stat"] == metrics.COUNT_STAT:
            value = str(int(round(float(row["value"]))))
        else:
            value = _baseline_value(row["value"])
        merged.append(
            {
                **{column: row[column] for column in REPORT_COLUMNS},
                "value": value,
                "ci_low": _baseline_value(row["ci_low"]),
                "ci_high": _baseline_value(row["ci_high"]),
                "recorded": recorded,
                "note": note,
            }
        )
    for row in existing:
        if str(row["cell"]) in covered:
            continue
        merged.append(row)
    merged.sort(
        key=lambda row: (
            row["cell"],
            row["family"],
            row["background"],
            row["subject"],
            metrics.STATS.index(row["stat"]) if row["stat"] in metrics.STATS else 0,
        )
    )
    return merged, new_metrics


def load_baseline_metadata(path) -> dict:
    """Per-cell identity and validity from a baselines file's `# cell:` lines."""
    metadata: dict = {}
    if not path or not Path(path).is_file():
        return metadata
    for line in Path(path).read_text().splitlines():
        if not line.startswith("# cell: "):
            continue
        try:
            payload = json.loads(line[len("# cell: ") :])
        except json.JSONDecodeError:
            continue
        identity = payload.get("identity") or {}
        cell = identity.get("cell")
        if cell:
            metadata[cell] = {
                "identity": identity,
                "validity": payload.get("validity") or {},
            }
    return metadata


def baselines_text(rows: list, env_lines: list, metadata: dict | None = None) -> str:
    """Baselines file: comments, environment, per-cell metadata, long rows."""
    metadata = metadata or {}
    body = [BASELINE_HEADER]
    current = None
    for row in rows:
        cell = row["cell"]
        if cell != current:
            current = cell
            block = metadata.get(cell)
            if block:
                body.append(_cell_line(block.get("identity") or {}, block.get("validity") or {}))
        body.append(_baseline_line(row))
    return "\n".join(list(_DEFAULT_BASELINE_COMMENTS) + list(env_lines) + body) + "\n"


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
    return (str(commit), str(record.get("started_utc", "")))


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
    """Merge accepted report rows from battery parents into baselines.tsv.

    The report is the single reduced record, so this step reads it instead
    of re-parsing rep artifacts: values and intervals cannot diverge from
    what the battery reported. Every metric of a battery run is recorded;
    debug cells are skipped. With `md_path` the
    Markdown document is rendered from the merged baselines.
    """
    from render_baselines import render

    parents, error = _resolve_parents(parents)
    if error is not None:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    accepted_rows: list = []
    metadata: dict = {}
    env_records: list = []
    rep_dirs: list = []
    for parent in parents:
        report_path = parent / "report.tsv"
        if not report_path.is_file():
            print(f"ERROR: {report_path}: no report.tsv (run the battery first?)", file=sys.stderr)
            return 1
        rep_dirs.extend(_rep_dirs(parent))
        env_record = environment.load_environment(parent / environment.ENVIRONMENT_FILENAME)
        if env_record is not None:
            env_records.append(env_record)
        for block in load_report(report_path):
            identity = block.get("identity") or {}
            if identity.get("source") != "battery":
                continue
            accepted_rows.extend(block["rows"])
            cell = identity.get("cell")
            if cell:
                metadata[cell] = {"identity": identity, "validity": block.get("validity") or {}}
    if not accepted_rows:
        print("ERROR: no report rows extracted", file=sys.stderr)
        return 2
    if merge_path is not None:
        merge_source = Path(merge_path)
    elif not (isinstance(tsv_path, str) and tsv_path == "-") and Path(tsv_path).is_file():
        merge_source = Path(tsv_path)
    else:
        merge_source = SCRIPT_DIR / "baselines.tsv"
    run_date = _run_date(rep_dirs)
    try:
        existing_rows = load_baselines(merge_source)
    except ValueError as err:
        print(f"warn: {err}; recording without prior baselines", file=sys.stderr)
        existing_rows = []
    rows, new_metrics = merge_baselines(
        accepted_rows,
        existing_rows,
        run_date=run_date,
        branch="",
        commit="",
    )
    existing_env = []
    if merge_source.is_file():
        existing_env = [
            line for line in merge_source.read_text().splitlines() if line.startswith("# env:")
        ]
    env_lines = merge_env_lines(existing_env, env_records)
    merged_metadata = load_baseline_metadata(merge_source)
    merged_metadata.update(metadata)
    text = baselines_text(rows, env_lines, merged_metadata)
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
        for cell, key in new_metrics:
            print(f"  {cell} {' '.join(key)}", file=sys.stderr)
    if md_path is not None:
        md_out = Path(md_path)
        md_out.parent.mkdir(parents=True, exist_ok=True)
        md_out.write_text(render(rows, merged_metadata, env_records))
        print(f"wrote baselines Markdown: {md_out}", file=sys.stderr)
    return 0


def extract_battery(parent: Path, out: Path | None = None) -> int:
    """Dump one battery parent as a single JSON document.

    Emits the parent's cell blocks (identity, validity and long rows) from
    `report.tsv`, or rebuilt from the rep artifacts when no report exists,
    plus the parent environment and an `errors` count. The JSON goes to
    `out` when given, otherwise to stdout; no other file is written.

    Baseline recording lives in `record_baselines`, not here.
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
    errors = []
    cells = []
    report_path = parent / "report.tsv"
    if report_path.is_file():
        try:
            cells = load_report(report_path)
        except ValueError as err:
            print(f"warn: {err}; rebuilding from rep artifacts", file=sys.stderr)
    if not cells:
        grouped: dict = {}
        for path in paths:
            entry = _extract_entry(path)
            if "error" in entry:
                errors.append({key: entry[key] for key in ("dir", "error")})
                continue
            row = entry["row"]
            grouped.setdefault(row.get("cell", ""), []).append(row)
        for group in grouped.values():
            rows, identity, validity = _pooled_report_rows(group)
            cells.append({"identity": identity, "validity": validity, "rows": rows})
    document = {
        "battery": str(parent),
        "cells": cells,
        "errors": errors,
        "environment": environment.load_environment(parent / environment.ENVIRONMENT_FILENAME),
    }
    payload = json.dumps(_json_safe(document), indent=2, allow_nan=False) + "\n"
    if out is not None:
        out_path = Path(out)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(payload)
    else:
        print(payload, end="")
    if not cells:
        print(f"ERROR: {parent}: no rep subdirectory parsed", file=sys.stderr)
        return 2
    return 0


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
    upload_repo = env.get("UPLOAD_REPO")
    if upload_repo and not Path(upload_repo).is_absolute():
        env["UPLOAD_REPO"] = str(Path(run_config.sets_root) / upload_repo)
    return env


def _reuse_row(cell: Cell, cell_out: Path) -> dict | None:
    """Return the report row of a completed rep, or None when it must run.

    run_cell writes `status-final.json` only for a terminal server state;
    a rep without that marker, or whose artifacts no longer parse, is
    incomplete and runs again.
    """
    cell_out = Path(cell_out)
    if not (cell_out / "status-final.json").is_file():
        return None
    try:
        return build_row(cell.profile, cell_out, cell_id=cell.cell_id, source=cell.source)
    except Exception, SystemExit:
        # Any parse failure marks the rep incomplete; a fresh run reports
        # its own row errors. KeyboardInterrupt still aborts the battery.
        return None


def run_battery(
    images: int,
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
    # The debug cell is fully parameterized by env: honor PORT/BUILD_PROFILE when
    # the caller did not override them explicitly.
    if build_profile == "release" and "BUILD_PROFILE" in environ:
        build_profile = config.validate_build_profile(environ, build_profile)
    if base_port == DEFAULT_BASE_PORT and "PORT" in environ:
        base_port = config.validate_port(environ["PORT"])
    run_config = config.BatteryConfig.from_env(
        environ,
        images=images,
        out=outdir,
        sets_root=sets_root,
        base_port=base_port,
        build_profile=build_profile,
        fresh=fresh,
    )
    n_repeats = run_config.repeats
    cells = battery_cells(images)
    report = outdir / "report.tsv"
    report.unlink(missing_ok=True)
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text("\t".join(REPORT_COLUMNS) + "\n")
    orchestrator = run_cell

    # Host snapshot for this battery run. On resume the original start is
    # kept and only the resume/completion fields move. Telemetry must never
    # fail the battery, so every touch below is best-effort.
    env_path = outdir / environment.ENVIRONMENT_FILENAME
    try:
        env_record = environment.load_environment(env_path)
        if env_record is None:
            env_record = environment.collect_environment(images=images)
            environment.write_environment(env_path, env_record)
        else:
            env_record["resumed_utc"] = environment.collect_environment(images=images)[
                "started_utc"
            ]
            environment.write_environment(env_path, env_record)
    except Exception as err:  # noqa: BLE001 - telemetry must not fail the battery
        print(f"[battery {images}] environment snapshot skipped: {err}", file=sys.stderr)

    try:
        for index, cell in enumerate(cells):
            repeat_rows = []
            min_probe_rounds = run_config.min_probe_rounds
            for rep in range(1, n_repeats + 1):
                cell_out = outdir / f"{cell.outdir_suffix}-rep{rep}"
                # Default resume: a completed rep is reused; --fresh re-runs it.
                row = None if fresh else _reuse_row(cell, cell_out)
                if row is not None:
                    repeat_rows.append(row)
                    print(
                        f"[battery {images}] {cell.cell_id}: rep={rep}/cap={n_repeats} reused {cell_out}",
                        flush=True,
                    )
                else:
                    env = _cell_env(environ, run_config, cell, index, cell_out)
                    print(
                        f"[battery {images}] {cell.cell_id}: rep={rep}/cap={n_repeats} port={env['PORT']} out={cell_out}",
                        flush=True,
                    )
                    cfg = orchestrator.config_from_env(env)
                    rc = orchestrator.run_cell(cfg, env)
                    try:
                        row = build_row(
                            cell.profile,
                            cell_out,
                            cell_id=cell.cell_id,
                            source=cell.source,
                        )
                    except (OSError, ValueError, SystemExit) as err:
                        print(
                            f"[battery {images}] {cell.cell_id}: no result row: {err}",
                            file=sys.stderr,
                        )
                        row = None
                    if row is not None:
                        repeat_rows.append(row)
                        print(
                            f"[battery {images}] {cell.cell_id}: row appended (rc={rc})", flush=True
                        )
                    if rc != 0:
                        print(
                            f"[battery {images}] {cell.cell_id}: cell failed rc={rc}",
                            file=sys.stderr,
                        )
                        return rc
                valid_rounds = 0
                for existing in repeat_rows:
                    value = (existing.get("validity") or {}).get("probe_rounds_valid", NA)
                    try:
                        valid_rounds += int(value)
                    except TypeError, ValueError:
                        continue
                has_probes = config.profile_has_probes(cell.profile)
                print(
                    f"[battery {images}] {cell.cell_id}: rep={rep}/cap={n_repeats} "
                    f"rounds={valid_rounds}/{min_probe_rounds}"
                    + ("" if has_probes else " (no probe phase)"),
                    flush=True,
                )
                if not has_probes or valid_rounds >= min_probe_rounds or rep >= n_repeats:
                    if has_probes and valid_rounds < min_probe_rounds and rep >= n_repeats:
                        print(
                            f"[battery {images}] {cell.cell_id}: warn: probe rounds "
                            f"{valid_rounds} below target {min_probe_rounds}",
                            file=sys.stderr,
                        )
                    break

            if repeat_rows:
                rows, identity, validity = _pooled_report_rows(repeat_rows)
                append_cell_rows(report, identity, validity, rows)
                print(
                    f"[battery {images}] {cell.cell_id}: {len(rows)} report rows "
                    f"(reps={validity.get('reps')}, rounds={validity.get('probe_rounds_valid')})",
                    flush=True,
                )
        print(f"[battery {images}] report: {report}", flush=True)
        return 0
    finally:
        # Stamp completion (started-/loadavg-at-start stay as recorded).
        try:
            done = environment.load_environment(env_path)
            if done is None:
                done = environment.collect_environment(images=images)
            environment.write_environment(env_path, environment.finish_environment(done))
        except Exception as err:  # noqa: BLE001 - telemetry must not fail the battery
            print(f"[battery {images}] environment finish skipped: {err}", file=sys.stderr)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    run = subparsers.add_parser("run", help="run a battery and write its report")
    run.add_argument("--images", type=int, default=None, help="image count for this battery")
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

    listing = subparsers.add_parser("list", help="print a battery's cell ids")
    listing.add_argument("--images", type=int, default=None)

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
        images = args.images
        if images is None:
            images = config.integer(os.environ, "IMAGES", config.DEFAULT_IMAGES, minimum=1)
        raw_out = args.out or os.environ.get("OUT", "")
        if raw_out == "":
            raw_out = f"/tmp/picasu-battery-{images}-{datetime.now():%Y%m%d-%H%M%S}"
        # SIGTERM unwinds through run_cell's finally so the launcher and its
        # server are stopped instead of orphaned between cells.
        signal.signal(signal.SIGTERM, _interrupt)
        try:
            return run_battery(
                images,
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

    if args.command == "extract":
        try:
            if args.baselines_tsv is not None or args.render_md is not None:
                if args.render_md is not None and args.baselines_tsv is None:
                    print("ERROR: --render-md needs --baselines-tsv", file=sys.stderr)
                    return 2
                if args.out is not None:
                    print("ERROR: --out takes exactly one battery parent", file=sys.stderr)
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
            )
        except (OSError, ValueError, ConfigError) as err:
            # A corrupt report.tsv surfaces the same way `check` reports it.
            print(f"ERROR: {err}", file=sys.stderr)
            return 1

    images = getattr(args, "images", None)
    if images is None:
        images = config.integer(os.environ, "IMAGES", config.DEFAULT_IMAGES, minimum=1)
    for cell in battery_cells(images):
        print(cell.cell_id)
    return 0


if __name__ == "__main__":
    sys.exit(main())
