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
import os
import re
import signal
import statistics
import sys
from dataclasses import dataclass
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_SETS_ROOT = Path("/tmp/sets")
DEFAULT_BASE_PORT = 5795
NA = "n/a"

# Per-level workload budget passed to run-cell as WORKLOAD_TIMEOUT_S:
# run_battery invokes run-cell in-process, so no outer timeout(1) bounds a
# cell the way the justfile recipe does. The scale figure covers the
# measured ~1335 s single control cell at 5650 rows.
WORKLOAD_TIMEOUT_S_BY_LEVEL = {"gate": 1800, "scale": 7200}

BATTERY_REPEATS_DEFAULT = 3
BATTERY_REPEATS_BY_LEVEL = {"gate": 3, "scale": 3}
BATTERY_REPEATS_MIN = 1

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


def battery_cells(level: str) -> tuple:
    """Return the ordered cell list for a battery level."""
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


def run_battery(
    level: str,
    outdir: Path,
    *,
    sets_root: Path = DEFAULT_SETS_ROOT,
    base_port: int = DEFAULT_BASE_PORT,
    build_profile: str = "release",
) -> int:
    """Run every cell sequentially, appending one row per cell."""
    outdir = Path(outdir).resolve()
    cells = battery_cells(level)
    outdir.mkdir(parents=True, exist_ok=True)
    report = outdir / "report.tsv"
    report.unlink(missing_ok=True)
    orchestrator = _run_cell()

    # Battery-level repeats: an exported BATTERY_REPEATS wins over the
    # per-level default (env.setdefault leaves an exported value intact).
    env0 = dict(os.environ)
    env0.setdefault(
        "BATTERY_REPEATS", str(BATTERY_REPEATS_BY_LEVEL.get(level, BATTERY_REPEATS_DEFAULT))
    )
    try:
        n_repeats = int(env0["BATTERY_REPEATS"])
    except TypeError, ValueError:
        raise ValueError(f"BATTERY_REPEATS must be an integer, got {env0['BATTERY_REPEATS']!r}")
    if n_repeats < BATTERY_REPEATS_MIN:
        raise ValueError(f"BATTERY_REPEATS must be >= {BATTERY_REPEATS_MIN}, got {n_repeats}")

    for index, cell in enumerate(cells):
        repeat_rows = []
        for rep in range(1, n_repeats + 1):
            cell_out = outdir / f"{cell.outdir_suffix}-rep{rep}"
            env = dict(os.environ)
            env.update(cell.env)
            env["BUILD_PROFILE"] = build_profile
            # run-cell's workload budget; an exported WORKLOAD_TIMEOUT_S wins.
            env.setdefault("WORKLOAD_TIMEOUT_S", str(WORKLOAD_TIMEOUT_S_BY_LEVEL[level]))
            env.setdefault("BATTERY_REPEATS", str(n_repeats))
            env.update(
                {
                    "OUT": str(cell_out),
                    "PORT": str(base_port + index),
                    "IMAGE_REPO": str(repo_path(cell, sets_root)),
                    "DATASET_ID": cell.repo,
                }
            )
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
    run.add_argument("level", choices=sorted(_BATTERIES))
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

    check = subparsers.add_parser("check", help="compare a report against baselines")
    check.add_argument("report", help="battery report TSV")
    check.add_argument(
        "--baselines",
        default=str(SCRIPT_DIR / "baselines.tsv"),
        help="baseline TSV (default: utils/bench/baselines.tsv)",
    )

    listing = subparsers.add_parser("list", help="print a battery's cell ids")
    listing.add_argument("level", choices=sorted(_BATTERIES))

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
            )
        except KeyboardInterrupt:
            print("[battery] interrupted", file=sys.stderr)
            return 130

    if args.command == "check":
        try:
            rows = load_report(Path(args.report))
            baselines = load_baselines(Path(args.baselines))
        except (OSError, ValueError) as err:
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

    for cell in battery_cells(args.level):
        print(cell.cell_id)
    return 0


if __name__ == "__main__":
    sys.exit(main())
