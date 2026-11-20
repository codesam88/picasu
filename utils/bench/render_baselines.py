#!/usr/bin/env python3
"""Render `baselines.md` from the checked-in baselines and the run reports.

The generated document owns the data content: the tables come from
`baselines.tsv` (accepted values) and the battery `report.tsv` files
(informational metrics and ranges). Terms are defined once up front;
nothing is read back from the target Markdown and no markers are
written into it.

Usage:
    utils/bench/render_baselines.py REPORT [REPORT ...]
        [--baselines utils/bench/baselines.tsv]
        [--out utils/bench/baselines.md]
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import battery  # noqa: E402

NA = battery.NA
CELL_ORDER = battery.CELL_ORDER
LEVEL_ORDER = ("micro", "gate", "scale")


def load_baseline_values(path: Path) -> dict:
    """Raw baseline strings, so displayed values keep their recorded form."""
    values = {}
    if not Path(path).is_file():
        return values
    for line in Path(path).read_text().splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        parts = line.split("\t")
        if parts[0] == "cell":
            continue
        if len(parts) != len(battery.BASELINE_COLUMNS):
            raise ValueError(f"{path}: malformed baselines row: {line}")
        values[(parts[0], parts[1])] = parts[2]
    return values


def load_reports(paths) -> dict:
    rows = {}
    for path in paths:
        for row in battery.load_report(path):
            rows[row["cell"]] = row
    return rows


def _pad(text: str, width: int, align: str) -> str:
    return text.rjust(width) if align == "right" else text.ljust(width)


def md_table(headers, aligns, rows) -> list:
    """One Markdown table in the layout prettier preserves."""
    rows = [[str(cell) for cell in row] for row in rows]
    widths = [
        max(3, len(headers[index]), *(len(row[index]) for row in rows))
        for index in range(len(headers))
    ]
    lines = [
        "| "
        + " | ".join(_pad(head, widths[i], aligns[i]) for i, head in enumerate(headers))
        + " |",
        "| "
        + " | ".join(
            ("-" * (widths[i] - 1) + ":") if aligns[i] == "right" else "-" * widths[i]
            for i in range(len(headers))
        )
        + " |",
    ]
    for row in rows:
        lines.append(
            "| " + " | ".join(_pad(cell, widths[i], aligns[i]) for i, cell in enumerate(row)) + " |"
        )
    return lines


def _level(cell: str, row) -> str:
    if row is not None and row.get("level") not in (None, "", NA):
        return row["level"]
    return cell.split("-", 1)[0]


def _cell_sort_key(cell: str):
    if cell in CELL_ORDER:
        return (0, CELL_ORDER[cell], cell)
    level = cell.split("-", 1)[0]
    rank = LEVEL_ORDER.index(level) if level in LEVEL_ORDER else len(LEVEL_ORDER)
    return (1, rank, cell)


def cells_for(level: str, report: dict, baseline_cells) -> list:
    cells = set(baseline_cells)
    cells.update(report)
    selected = [
        cell
        for cell in cells
        if cell.endswith(":median") and _level(cell, report.get(cell)) == level
    ]
    return sorted(selected, key=_cell_sort_key)


def _scale(cell: str, row) -> str:
    if row is not None and row.get("scale") not in (None, "", NA):
        return row["scale"]
    tail = cell.rsplit("-", 1)[-1].split(":", 1)[0]
    return tail if tail.isdigit() else cell


def _report_value(report, cell, metric) -> str:
    row = report.get(cell)
    if row is None:
        return NA
    value = row.get(metric, NA)
    return str(value) if value not in (None, "") else NA


def _value(baselines, report, cell, metric) -> str:
    recorded = baselines.get((cell, metric))
    if recorded is not None:
        return recorded
    return _report_value(report, cell, metric)


def _range(report, cell, metric) -> str:
    low = _report_value(report, cell, f"{metric}_min")
    high = _report_value(report, cell, f"{metric}_max")
    if low == NA or high == NA:
        return NA
    return f"{low}-{high}"


def load_env_records(path: Path) -> list:
    """Environment records from `# env: {...}` comment lines, in file order."""
    import json

    records = []
    try:
        lines = Path(path).read_text().splitlines()
    except OSError:
        return records
    for line in lines:
        if not line.startswith("# env:"):
            continue
        try:
            record = json.loads(line[len("# env:") :])
        except ValueError:
            continue
        if isinstance(record, dict):
            records.append(record)
    return records


def _env_cell(value) -> str:
    return "unknown" if value is None else str(value)


def _env_mhz(value) -> str:
    return "unknown" if value is None or value == "unknown" else str(value)


def _env_group_key(run):
    host = run.get("hostname") if run.get("hostname") is not None else run.get("host")
    return (host,) + tuple(
        run.get(key)
        for key in (
            "kernel",
            "cpu_model",
            "cpu_count",
            "freq_cur_mhz",
            "freq_max_mhz",
            "governor",
            "turbo_disabled",
            "mem_total_kb",
        )
    )


def _env_time(group, key, latest=False) -> str:
    values = [str(run.get(key)) for run in group if run.get(key) not in (None, "unknown")]
    if not values:
        return "unknown"
    return max(values) if latest else min(values)


def _env_bullets(runs) -> list:
    groups = {}
    for run in runs:
        groups.setdefault(_env_group_key(run), []).append(run)
    lines = []
    for group in groups.values():
        if lines:
            lines.append("")
        first = group[0]
        get = first.get
        host = get("hostname") if get("hostname") is not None else get("host")
        freq = f"{_env_mhz(get('freq_cur_mhz'))}/{_env_mhz(get('freq_max_mhz'))} MHz"
        mem_kb = get("mem_total_kb")
        mem = f"{mem_kb // 1048576} GiB" if isinstance(mem_kb, int) else _env_cell(mem_kb)
        start_load = "/".join(_env_cell(get(f"load_{n}")) for n in ("1", "5", "15"))
        end_load = "/".join(_env_cell(get(f"end_load_{n}")) for n in ("1", "5", "15"))
        turbo = get("turbo_disabled")
        turbo_text = (
            "unknown"
            if turbo is None or turbo == "unknown"
            else ("disabled" if turbo else "enabled")
        )
        commits = sorted(
            {
                f"{_env_cell(run.get('git_branch'))} @ {str(run.get('git_commit', 'unknown'))[:12]}"
                for run in group
            }
        )
        lines += [
            f"- **host**: {_env_cell(host)} ({_env_cell(get('kernel'))})",
            f"- **CPU**: {_env_cell(get('cpu_model'))}, {_env_cell(get('cpu_count'))} cores, "
            f"{freq}, frequency governor {_env_cell(get('governor'))}, "
            f"turbo boost {turbo_text}",
            f"- **memory**: {mem}",
            f"- **load average (1/5/15 min)**: {start_load} at start, {end_load} at end",
            f"- **git**: {', '.join(commits)}",
            f"- **captured**: {_env_time(group, 'started_utc')} to "
            f"{_env_time(group, 'ended_utc', latest=True)}",
        ]
    return lines


_SCENARIO_ORDER = (
    "control",
    "storm-read",
    "storm-thumb",
    "storm-diff",
    "storm-idle",
    "upload",
)
_SCENARIO_LABELS = {
    "control": "control",
    "storm-read": "read burst",
    "storm-thumb": "thumbnail burst",
    "storm-diff": "edit burst",
    "storm-idle": "read burst (idle)",
    "upload": "upload",
}
_BURST_ROWS = {
    "storm-read": ("scan", "read"),
    "storm-thumb": ("scan", "thumbnail"),
    "storm-diff": ("scan", "edit"),
    "storm-idle": ("idle", "read"),
}
_BURST_KINDS = {"read": "read", "thumbnail": "thumb", "edit": "edit"}
_IDLE_ACCESSES = (
    ("read", "idle_prefetch", True),
    ("thumbnail", "idle_thumb", True),
    ("edit", "idle_edit", True),
    ("scroll bar", "scroll_bar", False),
    ("scroll rows", "scroll_rows", False),
    ("scroll data", "scroll_data", False),
)
_SCAN_ACCESSES = (
    ("read", "scan_prefetch"),
    ("thumbnail", "scan_thumb"),
    ("edit", "scan_edit"),
)
_USCAN_ACCESSES = (
    ("read", "uscan_prefetch"),
    ("thumbnail", "uscan_thumb"),
    ("edit", "uscan_edit"),
)


def _profile_of(report, cell) -> str:
    return (report.get(cell) or {}).get("profile", "")


def _images_value(cell, report):
    images = _scale(cell, report.get(cell))
    return int(images) if images.isdigit() else None


def _scenario_index(cell, report) -> int:
    profile = _profile_of(report, cell)
    return _SCENARIO_ORDER.index(profile) if profile in _SCENARIO_ORDER else len(_SCENARIO_ORDER)


def _ordered_cells(cells, report) -> list:
    def sort_key(cell):
        images = _images_value(cell, report)
        return (images is None, images or 0, _scenario_index(cell, report), cell)

    return sorted(cells, key=sort_key)


def _scenario_label(cell, row) -> str:
    profile = (row or {}).get("profile", "")
    return _SCENARIO_LABELS.get(profile, profile or cell)


def _footprint_rows(cells, report) -> list:
    return [
        [
            _scale(cell, report.get(cell)),
            _scenario_label(cell, report.get(cell)),
            _report_value(report, cell, "vm_hwm_mb"),
            _report_value(report, cell, "temp_db_mb"),
            _report_value(report, cell, "index_v5_mb"),
            _report_value(report, cell, "cache_db_mb"),
            _report_value(report, cell, "compressed_mb"),
        ]
        for cell in _ordered_cells(cells, report)
    ]


def render(report: dict, baselines: dict, env_runs=()) -> str:
    baseline_cells = {cell for cell, _metric in baselines}
    lines = [
        "# Benchmark baselines",
        "",
        "Generated from `baselines.tsv` and the battery `report.tsv` files. "
        "`battery check` gates the baselined medians; context values (idle "
        "worst cases, scroll, bursts, memory footprint) come from the latest "
        "report.",
        "",
        "## Terms",
        "",
        "- **#images**: number of images in the scanned set.",
        "- **scenario**: the server load a run applies: control (no extra load), "
        "a synthetic burst, or upload.",
        "- **idle**: server at rest after the scan; the idle accesses are the baseline.",
        "- **scan**: while the initial indexing scan of the image set runs.",
        "- **upload**: while files are continuously uploaded and scanned.",
        "- **burst**: a synthetic client loop repeating one access on top of the idle "
        "or scan state.",
        "- **access**: the API action measured: read (grid prefetch), thumbnail "
        "(download), edit (metadata), scroll bar/rows/data (scroll requests), upload "
        "submit/visible (upload endpoints).",
        "- **median**: median over repeat runs.",
        "- **worst_case**: worst observed repeat value.",
        "- **wall time**: elapsed wall-clock time of the scan.",
        "- **tree build**: album tree rebuild stage inside the scan.",
        "- **filter clone**: filter snapshot clone stage inside the scan.",
        "- **peak memory**: peak resident memory of the server process.",
        "- **DB**: on-disk database; index, cache, and temp are separate stores.",
        "- **compressed store**: on-disk store of generated image files.",
        "- **units**: ms = milliseconds, us = microseconds, s = seconds, "
        "MB = 10^6 bytes, MiB = 2^20 bytes, GiB = 2^30 bytes.",
    ]
    if env_runs:
        lines += [
            "",
            "## Run environment",
            "",
            "Host environment of the recorded baselines, captured once per "
            "battery run; `unknown` marks fields the collection predates.",
            "",
        ]
        lines += _env_bullets(env_runs)

    cells = cells_for("micro", report, baseline_cells) + cells_for("gate", report, baseline_cells)
    if cells:
        lines += [
            "",
            "## Memory footprint",
            "",
            "Peak process memory and on-disk store sizes after the run.",
            "",
        ]
        lines += md_table(
            [
                "#images",
                "scenario",
                "peak memory MiB",
                "temp DB MB",
                "index DB MB",
                "cache DB MB",
                "compressed store MB",
            ],
            ["right", "left", "right", "right", "right", "right", "right"],
            _footprint_rows(cells, report),
        )

    control = _ordered_cells(
        [cell for cell in cells if _profile_of(report, cell) == "control"], report
    )
    if control:
        lines += [
            "",
            "## Idle access latency",
            "",
            "Server at rest after the scan; these accesses are the baseline.",
            "",
        ]
        rows = []
        for cell in control:
            for label, key, gated in _IDLE_ACCESSES:
                median = (
                    _value(baselines, report, cell, f"{key}_p50_ms")
                    if gated
                    else _report_value(report, cell, f"{key}_p50_ms")
                )
                rows.append(
                    [
                        _scale(cell, report.get(cell)),
                        label,
                        median,
                        _report_value(report, cell, f"{key}_max_ms"),
                    ]
                )
        lines += md_table(
            ["#images", "access", "median ms", "worst_case ms"],
            ["right", "left", "right", "right"],
            rows,
        )

        lines += ["", "## Scan access latency", "", "While the initial indexing scan runs.", ""]
        lines += md_table(
            ["#images", "access", "median ms", "worst_case ms"],
            ["right", "left", "right", "right"],
            [
                [
                    _scale(cell, report.get(cell)),
                    label,
                    _value(baselines, report, cell, f"{key}_p50_ms"),
                    _value(baselines, report, cell, f"{key}_max_ms"),
                ]
                for cell in control
                for label, key in _SCAN_ACCESSES
            ],
        )

    upload = _ordered_cells(
        [cell for cell in cells if _profile_of(report, cell) == "upload"], report
    )
    if upload:
        lines += [
            "",
            "## Upload access latency",
            "",
            "While files are continuously uploaded and scanned.",
            "",
        ]
        rows = [
            [
                _scale(cell, report.get(cell)),
                label,
                _value(baselines, report, cell, f"{key}_p50_ms"),
                _value(baselines, report, cell, f"{key}_max_ms"),
            ]
            for cell in upload
            for label, key in _USCAN_ACCESSES
        ]
        rows += [
            [
                _scale(cell, report.get(cell)),
                label,
                _value(baselines, report, cell, key),
                NA,
            ]
            for cell in upload
            for label, key in (
                ("upload submit", "upload_submit_p50_ms"),
                ("upload visible", "upload_visible_p50_ms"),
            )
        ]
        lines += md_table(
            ["#images", "access", "median ms", "worst_case ms"],
            ["right", "left", "right", "right"],
            rows,
        )

    bursts = _ordered_cells(
        [cell for cell in cells if _profile_of(report, cell) in _BURST_ROWS], report
    )
    if bursts:
        lines += [
            "",
            "## Burst access latency",
            "",
            "A synthetic client burst of one access type on top of the idle or scan state.",
            "",
        ]
        rows = []
        for cell in bursts:
            background, access = _BURST_ROWS[_profile_of(report, cell)]
            prefix = f"{background}_storm_{_BURST_KINDS[access]}"
            rows.append(
                [
                    _scale(cell, report.get(cell)),
                    f"{background} + {access} burst",
                    access,
                    _report_value(report, cell, f"{prefix}_p50_ms"),
                    _report_value(report, cell, f"{prefix}_max_ms"),
                    _report_value(report, cell, f"{prefix}_failed"),
                ]
            )
        lines += md_table(
            [
                "#images",
                "scenario",
                "access",
                "median ms",
                "worst_case ms",
                "failed requests",
            ],
            ["right", "left", "left", "right", "right", "right"],
            rows,
        )

    if cells:
        lines += [
            "",
            "## Scan wall time and stages",
            "",
            "Wall time and the two dominant internal scan stages.",
            "",
        ]
        rows = []
        for cell in _ordered_cells(cells, report):
            wall = _value(baselines, report, cell, "scan_wall_s")
            span = _range(report, cell, "scan_wall_s")
            rows.append(
                [
                    _scale(cell, report.get(cell)),
                    _scenario_label(cell, report.get(cell)),
                    wall if span == NA else f"{wall} ({span})",
                    _value(baselines, report, cell, "tree_refresh_build_p50_us"),
                    _value(baselines, report, cell, "tree_refresh_build_max_us"),
                    _value(baselines, report, cell, "filter_snapshot_clone_p50_us"),
                    _value(baselines, report, cell, "filter_snapshot_clone_max_us"),
                ]
            )
        lines += md_table(
            [
                "#images",
                "scenario",
                "wall time s (min-max)",
                "tree build median us",
                "tree build worst_case us",
                "filter clone median us",
                "filter clone worst_case us",
            ],
            ["right", "left", "left", "right", "right", "right", "right"],
            rows,
        )

    return "\n".join(lines) + "\n"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reports", nargs="+", type=Path, help="battery report.tsv files")
    parser.add_argument(
        "--baselines",
        type=Path,
        default=SCRIPT_DIR / "baselines.tsv",
        help="accepted baselines (default: utils/bench/baselines.tsv)",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=SCRIPT_DIR / "baselines.md",
        help="Markdown output (default: utils/bench/baselines.md)",
    )
    args = parser.parse_args(argv)
    try:
        baselines = load_baseline_values(args.baselines)
        report = load_reports(args.reports)
        runs = load_env_records(args.baselines)
    except (OSError, ValueError) as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 1
    args.out.write_text(render(report, baselines, runs))
    print(f"wrote {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
