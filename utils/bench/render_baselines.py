#!/usr/bin/env python3
"""Render `baselines.md` from accepted baselines and their cell metadata."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import battery  # noqa: E402
import metrics  # noqa: E402

NA = battery.NA
ACCESSES = metrics.ACCESSES
GAUGES = ("vm_hwm_mb", "temp_db_mb", "index_v5_mb", "cache_db_mb", "compressed_mb")


def _rows(value):
    return [row for row in battery.load_baselines(value)] if not isinstance(value, list) else value


def _one_decimal(value):
    if value in (None, "", NA):
        return NA
    try:
        number = float(value)
    except TypeError, ValueError:
        return str(value)
    return f"{number:.1f}"


def _lookup(rows, cell, family, background, subject, stat):
    for row in rows:
        if (
            row.get("cell") == cell
            and row.get("family") == family
            and row.get("background") == background
            and row.get("subject") == subject
            and row.get("stat") == stat
        ):
            return row
    return None


def _p50_text(rows, cell, family, background, subject):
    """`value ±half-width`; the interval is always shown, even when zero."""
    row = _lookup(rows, cell, family, background, subject, "p50")
    if row is None:
        return NA
    value = _one_decimal(row.get("value"))
    if value == NA:
        return NA
    low = _one_decimal(row.get("ci_low"))
    high = _one_decimal(row.get("ci_high"))
    if low == NA or high == NA:
        return f"{value} ±n/a"
    return f"{value} ±{(float(high) - float(low)) / 2:.1f}"


def _stat_text(rows, cell, family, background, subject, stat):
    row = _lookup(rows, cell, family, background, subject, stat)
    if row is None:
        return NA
    return _one_decimal(row.get("value"))


def _gauge(rows, cell, subject):
    row = _lookup(rows, cell, "gauge", "idle", subject, "p50")
    return _one_decimal(row.get("value")) if row else NA


def _cell_info(metadata, cell):
    identity = (metadata.get(cell) or {}).get("identity") or {}
    images = identity.get("images")
    profile = identity.get("profile")
    if images is None or profile is None:
        head, separator, tail = str(cell).rpartition("-")
        if separator and tail.isdigit():
            images = int(tail) if images is None else images
            profile = profile or head
    return {"cell": cell, "images": images, "profile": profile or str(cell)}


def _by_images(rows, metadata):
    """`{images: [profile, ...]}` from the cells that have baseline rows."""
    groups: dict = {}
    seen = set()
    for row in rows:
        cell = row.get("cell")
        if cell in seen:
            continue
        seen.add(cell)
        info = _cell_info(metadata, cell)
        groups.setdefault(info["images"], []).append(info["profile"])
    return {images: sorted(profiles) for images, profiles in groups.items()}


def _cell_for(metadata, rows, images, profile):
    for row in rows:
        cell = row.get("cell")
        info = _cell_info(metadata, cell)
        if info["images"] == images and info["profile"] == profile:
            return cell
    return None


def _md_table(headers, aligns, rows):
    if not rows:
        return []
    rows = [[str(cell) for cell in row] for row in rows]
    widths = [max(3, len(headers[i]), *(len(row[i]) for row in rows)) for i in range(len(headers))]
    lines = [
        "| "
        + " | ".join(
            (text.rjust(widths[i]) if aligns[i] == "right" else text.ljust(widths[i]))
            for i, text in enumerate(headers)
        )
        + " |",
        "| "
        + " | ".join(
            ("-" * (widths[i] - 1) + ":") if aligns[i] == "right" else (":" + "-" * (widths[i] - 1))
            for i in range(len(headers))
        )
        + " |",
    ]
    for row in rows:
        lines.append(
            "| "
            + " | ".join(
                (text.rjust(widths[i]) if aligns[i] == "right" else text.ljust(widths[i]))
                for i, text in enumerate(row)
            )
            + " |"
        )
    return lines


def _images_sections(groups):
    return sorted(images for images in groups if images is not None)


def _load(value):
    try:
        return f"{float(value):.2f}"
    except TypeError, ValueError:
        return str(value if value is not None else "?")


def load_env_records(path):
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


def _env_bullets(runs):
    lines = []
    for run in runs:
        if not isinstance(run, dict):
            lines += [f"- {run}", ""]
            continue
        turbo = run.get("turbo_disabled")
        turbo_text = (
            "unknown" if turbo in (None, "unknown") else ("disabled" if turbo else "enabled")
        )
        try:
            memory = f"{round(int(run.get('mem_total_kb', 0)) / 1048576)} GiB"
        except TypeError, ValueError:
            memory = "unknown"
        commit = str(run.get("git_commit", "") or "")[:12]
        lines += [
            f"- **host**: {run.get('hostname', 'unknown')} ({run.get('kernel', 'unknown')})",
            f"- **CPU**: {run.get('cpu_model', 'unknown')}, {run.get('cpu_count', '?')} cores, "
            f"{run.get('freq_cur_mhz', '?')}/{run.get('freq_max_mhz', '?')} MHz, "
            f"governor {run.get('governor', 'unknown')}, turbo {turbo_text}",
            f"- **memory**: {memory}",
            f"- **load average (1/5/15 min)**: "
            f"{_load(run.get('load_1'))}/{_load(run.get('load_5'))}/{_load(run.get('load_15'))} at start, "
            f"{_load(run.get('end_load_1'))}/{_load(run.get('end_load_5'))}/{_load(run.get('end_load_15'))} at end",
            f"- **git**: {run.get('git_branch', 'unknown')} @ {commit or 'unknown'}",
            f"- **captured**: {run.get('started_utc', 'unknown')} to {run.get('ended_utc', 'unknown')}",
            "",
        ]
    return lines


def render(baseline_rows, metadata, env_records):
    rows = _rows(baseline_rows)
    metadata = metadata or {}
    groups = _by_images(rows, metadata)
    images_list = _images_sections(groups)
    lines = ["# Benchmark baselines", ""]

    lines += [
        "## Run environment",
        "",
        "Host and build of the recorded runs. Compare only runs from the same "
        "machine and build: `16n` client counts are resolved from the recorded "
        "CPU count.",
        "",
    ]
    lines += _env_bullets(env_records) if env_records else ["No environment records.", ""]

    lines += [
        "## Measurement validity",
        "",
        "Probe rounds are requests issued and completed while a scan ran; "
        "`probe coverage` is the share of scan wall they cover and "
        "`storm coverage` the share covered by a concurrent burst. Low coverage "
        "or a non-zero `invalid` count means the latency rows describe only part "
        "of the scan; `failed` counts operations that did not return 200.",
        "",
    ]
    for images in images_list:
        lines += [f"### {images} images", ""]
        table = []
        for profile in groups[images]:
            cell = _cell_for(metadata, rows, images, profile)
            validity = (metadata.get(cell) or {}).get("validity") or {}
            table.append(
                [
                    profile,
                    validity.get("reps", NA),
                    validity.get("probe_rounds_valid", NA),
                    _one_decimal(validity.get("probe_coverage_pct", NA)),
                    _one_decimal(validity.get("storm_coverage_pct", NA)),
                    validity.get("warmup_valid", NA),
                    validity.get("phase_invalid", NA),
                    validity.get("failed", NA),
                ]
            )
        lines += _md_table(
            [
                "profile",
                "reps",
                "valid rounds",
                "probe coverage %",
                "storm coverage %",
                "warmup",
                "invalid",
                "failed",
            ],
            ["left", "right", "right", "right", "right", "left", "right", "right"],
            table,
        )
        lines.append("")

    def latency_section(title, intro, background, family="latency", keep=None):
        lines_local = [f"## {title}", "", intro, ""]
        for images in images_list:
            table = []
            for profile in groups[images]:
                if keep is not None and not keep(profile):
                    continue
                cell = _cell_for(metadata, rows, images, profile)
                for subject in ACCESSES:
                    p50 = _p50_text(rows, cell, family, background, subject)
                    p95 = _stat_text(rows, cell, family, background, subject, "p95")
                    maximum = _stat_text(rows, cell, family, background, subject, "max")
                    if p50 == NA and p95 == NA and maximum == NA:
                        continue
                    table.append([profile, subject, p50, p95, maximum])
            if not table:
                continue
            lines_local += [f"### {images} images", ""]
            lines_local += _md_table(
                ["profile", "access", "p50 ms ± CI", "p95", "max"],
                ["left", "left", "left", "right", "right"],
                table,
            )
            lines_local.append("")
        return lines_local

    lines += latency_section(
        "Idle access latency",
        "Requests served with the server at rest after the scan: the serving "
        "floor. Compare p50 across builds; a p95 or max move without a p50 move "
        "points at tail behaviour rather than steady state.",
        "idle",
        keep=lambda profile: not profile.startswith("storm-"),
    )
    lines += latency_section(
        "During-scan access latency",
        "The same accesses while an album scan runs. The gap to idle is the "
        "indexing-contention cost and the primary regression signal for the "
        "request path.",
        "scan",
        keep=lambda profile: not profile.startswith("storm-"),
    )
    lines += [
        "## Concurrent load",
        "",
        "Sustained load at the client count named by the storm profile, during a "
        "scan or against the tree at rest. p50 is loaded serving latency; p95 and "
        "max expose queueing tails. Only the profile's storm access is shown, so "
        "the rows are comparable across client counts.",
        "",
    ]
    for images in images_list:
        table = []
        for profile in groups[images]:
            cell = _cell_for(metadata, rows, images, profile)
            if not profile.startswith("storm-"):
                continue
            for background in ("scan", "idle"):
                access = (
                    profile.split("-")[1]
                    if not profile.startswith("storm-idle")
                    else profile.split("-")[2]
                )
                access = {"read": "prefetch", "thumb": "thumb", "edit": "edit"}.get(access, access)
                p50 = _p50_text(rows, cell, "latency", background, access)
                p95 = _stat_text(rows, cell, "latency", background, access, "p95")
                maximum = _stat_text(rows, cell, "latency", background, access, "max")
                if p50 == NA and p95 == NA and maximum == NA:
                    continue
                table.append([profile, background, access, p50, p95, maximum])
        if not table:
            continue
        lines += [f"### {images} images", ""]
        lines += _md_table(
            ["profile", "background", "access", "p50 ms ± CI", "p95", "max"],
            ["left", "left", "left", "left", "right", "right"],
            table,
        )
        lines.append("")

    lines += [
        "## Scan wall and stages",
        "",
        "How long populate scans took and where the time went, from the observer "
        "drain table. Wall well above the sum of the stage rows points at work "
        "outside the instrumented stages or at permit waits.",
        "",
    ]
    for images in images_list:
        table = []
        for profile in groups[images]:
            cell = _cell_for(metadata, rows, images, profile)
            p50 = _p50_text(rows, cell, "wall", "scan", "scan")
            if p50 == NA:
                continue
            table.append(
                [
                    profile,
                    p50,
                    _stat_text(rows, cell, "wall", "scan", "scan", "p95"),
                    _stat_text(rows, cell, "wall", "scan", "scan", "max"),
                ]
            )
        if table:
            lines += [f"### {images} images", ""]
            lines += _md_table(
                ["profile", "p50 s ± CI", "p95", "max"],
                ["left", "left", "right", "right"],
                table,
            )
            lines.append("")
        stages = sorted(
            {
                row.get("subject")
                for row in rows
                if row.get("family") == "stage"
                and _cell_info(metadata, row.get("cell"))["images"] == images
            }
        )
        stage_table = []
        for profile in groups[images]:
            cell = _cell_for(metadata, rows, images, profile)
            for stage in stages:
                p50 = _p50_text(rows, cell, "stage", "scan", stage)
                p95 = _stat_text(rows, cell, "stage", "scan", stage, "p95")
                maximum = _stat_text(rows, cell, "stage", "scan", stage, "max")
                if p50 == NA and p95 == NA and maximum == NA:
                    continue
                stage_table.append([profile, stage, p50, p95, maximum])
        if stage_table:
            lines += _md_table(
                ["profile", "stage", "p50 us ± CI", "p95", "max"],
                ["left", "left", "left", "right", "right"],
                stage_table,
            )
            lines.append("")

    lines += [
        "## Upload",
        "",
        "Write path end to end: submit-to-accept and time until the uploaded row "
        "is served. `visible` includes the indexing drain the upload triggers, so "
        "it tracks ingest plus scan progress.",
        "",
    ]
    for images in images_list:
        table = []
        for profile in groups[images]:
            cell = _cell_for(metadata, rows, images, profile)
            for subject in ("submit", "visible"):
                p50 = _p50_text(rows, cell, "upload", "idle", subject)
                if p50 == NA:
                    continue
                table.append(
                    [
                        profile,
                        subject,
                        p50,
                        _stat_text(rows, cell, "upload", "idle", subject, "p95"),
                        _stat_text(rows, cell, "upload", "idle", subject, "max"),
                    ]
                )
        if table:
            lines += [f"### {images} images", ""]
            lines += _md_table(
                ["profile", "phase", "p50 ms ± CI", "p95", "max"],
                ["left", "left", "left", "right", "right"],
                table,
            )
            lines.append("")

    lines += [
        "## Memory and storage footprint",
        "",
        "Peak process memory and store sizes at the end of each cell, one row per "
        "configuration, sorted by peak memory. Use it to spot allocations that "
        "scale with image count or scenario rather than with the measured work; a "
        "row far above its neighbours is worth a second look.",
        "",
    ]
    memory = []
    for images in images_list:
        for profile in groups[images]:
            cell = _cell_for(metadata, rows, images, profile)
            values = [_gauge(rows, cell, gauge) for gauge in GAUGES]
            if all(value == NA for value in values):
                continue
            memory.append([images, profile, *values])
    memory.sort(
        key=lambda item: float(item[2]) if item[2] != NA else -1.0,
        reverse=True,
    )
    lines += _md_table(
        [
            "images",
            "profile",
            "peak MiB",
            "temp_db MB",
            "index DB MB",
            "cache DB MB",
            "compressed MB",
        ],
        ["right", "left", "right", "right", "right", "right", "right"],
        memory,
    )
    lines.append("")

    lines += [
        "## Failed operations",
        "",
        "Operations that did not return 200. A non-zero count makes that cell's "
        "latency rows suspect.",
        "",
    ]
    for images in images_list:
        table = []
        for profile in groups[images]:
            cell = _cell_for(metadata, rows, images, profile)
            for row in rows:
                if (
                    row.get("cell") == cell
                    and row.get("family") == "count"
                    and row.get("stat") == metrics.COUNT_STAT
                ):
                    table.append(
                        [profile, row.get("background"), row.get("subject"), row.get("value")]
                    )
        if table:
            lines += [f"### {images} images", ""]
            lines += _md_table(
                ["profile", "background", "access", "failed"],
                ["left", "left", "left", "right"],
                table,
            )
            lines.append("")

    lines += [
        "## Terms",
        "",
        "- **p50**: median of the pooled samples for that condition.",
        "- **p95**: 95th percentile of the same pooled samples.",
        "- **max**: largest observed sample.",
        "- **± CI**: half the width of the seeded 95% bootstrap interval, shown "
        "next to p50 only; `±n/a` means fewer than two samples.",
        "- **profile**: the cell's scenario, including its client count for storms.",
        "- **coverage**: valid measurement wall as a share of scan wall.",
        "- **peak MiB**: peak resident set; the other footprint columns are "
        "apparent store sizes in decimal MB.",
        "",
    ]
    return "\n".join(lines)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
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
        baselines = _rows(args.baselines)
        metadata = battery.load_baseline_metadata(args.baselines)
        runs = load_env_records(args.baselines)
    except (OSError, ValueError) as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 1
    args.out.write_text(render(baselines, metadata, runs))
    print(f"wrote {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
