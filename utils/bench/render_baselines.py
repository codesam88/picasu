#!/usr/bin/env python3
"""Render `baselines.md` from accepted baselines and their cell metadata."""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import battery  # noqa: E402
import metrics  # noqa: E402
import scenarios  # noqa: E402

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


def _accesses(rows, cell, background):
    """Timed foreground operations pooled for one background of a cell.

    A latency sample is one request; an upload's `submit` sample is one file.
    `visible` observes the same upload again, so it is not counted twice.
    """
    total = 0
    found = False
    for row in rows:
        if (
            row.get("cell") != cell
            or row.get("stat") != "p50"
            or row.get("background") != background
        ):
            continue
        family = row.get("family")
        if family == "latency" or (
            family == "upload" and row.get("subject") == "submit"
        ):
            try:
                total += int(row.get("n_samples"))
            except TypeError, ValueError:
                continue
            found = True
    return total if found else NA


def _validity_value(metadata, cell, field):
    validity = (metadata.get(cell) or {}).get("validity") or {}
    return validity.get(field, NA)


INVALID_WARNING_RATE = 0.10


def _suspect_reasons(metadata, cell):
    """Why a cell's numbers deserve a WARNING, or [] when they do not.

    Guard rejections beyond 10% of the cell's measured probe rounds — the
    only phase a guard can reject without failing the cell at runtime — or
    any failed operation qualify. A rejection without a denominator
    qualifies as well.
    """
    if cell is None:
        return []
    validity = (metadata.get(cell) or {}).get("validity") or {}
    reasons = []
    failed = validity.get("failed")
    if isinstance(failed, (int, float)) and not isinstance(failed, bool) and failed > 0:
        reasons.append(f"{int(failed)} failed operation(s)")
    invalid = validity.get("phase_invalid")
    if not (isinstance(invalid, (int, float)) and not isinstance(invalid, bool) and invalid > 0):
        return reasons
    total = validity.get("probe_rounds_total")
    if isinstance(total, (int, float)) and not isinstance(total, bool) and total > 0:
        if invalid / total > INVALID_WARNING_RATE:
            reasons.append(
                f"{int(invalid)}/{int(total)} guard rejections exceed the 10% budget"
            )
    else:
        reasons.append(f"{int(invalid)} guard rejection(s) without a recorded round count")
    return reasons


def _warn_suspect_cells(metadata):
    """One stderr WARNING per cell past the 10% rejection budget."""
    for cell in sorted(metadata):
        reasons = _suspect_reasons(metadata, cell)
        if reasons:
            print(f"WARNING: cell {cell}: {'; '.join(reasons)}", file=sys.stderr)


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


def _storm_parts(profile):
    """(background, kind, level) encoded by a storm profile name."""
    if scenarios.is_storm(profile):
        return scenarios.storm_parts(profile)
    return None, None, None


def _probe_cell(groups, metadata, rows, images):
    """The probe cell for one image count."""
    if scenarios.PROBE not in groups[images]:
        return None, None
    return scenarios.PROBE, _cell_for(metadata, rows, images, scenarios.PROBE)


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


# Fields that must agree across every recorded run before one report can
# describe them. Clock frequency and load are expected to vary.
_ENV_IDENTITY = (
    "hostname",
    "kernel",
    "cpu_model",
    "cpu_count",
    "mem_total_kb",
    "git_branch",
    "git_commit",
)


def _environment(runs):
    """The earliest environment snapshot, after checking the runs agree.

    A report only makes sense when every recorded run shares host, CPU,
    memory and git identity. Missing fields are not compared; a mismatch
    refuses the document.
    """
    records = [run for run in (runs or []) if isinstance(run, dict)]
    if not records:
        return None
    ordered = sorted(records, key=lambda run: str(run.get("started_utc") or ""))
    first = ordered[0]
    for other in ordered[1:]:
        for field in _ENV_IDENTITY:
            left = first.get(field)
            right = other.get(field)
            if left in (None, "unknown", "") or right in (None, "unknown", ""):
                continue
            if left != right:
                raise ValueError(
                    f"environment mismatch on {field}: {left!r} vs {right!r}; "
                    "the runs cannot share one report"
                )
    return first


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
    _warn_suspect_cells(metadata)
    images_list = _images_sections(groups)
    lines = ["# Benchmark baselines", ""]

    lines += [
        "## Run environment",
        "",
        "Host and build of the recorded runs, printed once from the earliest "
        "run after checking every run agrees on host, CPU, memory and git "
        "identity; clock frequency and load are not compared. `16n` client "
        "counts are resolved from the recorded CPU count.",
        "",
    ]
    environment = _environment(env_records)
    lines += _env_bullets([environment]) if environment else ["No environment records.", ""]

    lines += [
        "## Terms",
        "",
        "- **p50**: median of the pooled samples for that condition.",
        "- **p95**: 95th percentile of the same pooled samples.",
        "- **max**: largest observed sample.",
        "- **± CI**: half the width of the seeded 95% bootstrap interval, shown "
        "next to p50 only; `±n/a` means fewer than two samples.",
        "- **background**: `idle` (no scan, server settled) or `scan` (an album scan is running).",
        "- **foreground**: the access being timed (read, edit, thumbnail, scroll, upload, storm).",
        "- **profile**: the execution cell that produces the foreground samples; "
        "storm profiles also name the kind and client count.",
        "- **peak VMM**: process memory high-water mark, shown in MB like the "
        "store sizes.",
        "",
    ]

    lines += [
        "## Measurement validity",
        "",
        "One row per load profile with the at-rest and scanning measurements "
        "side by side. `accesses` is the number of timed foreground operations "
        "pooled for that background — every probe request, every storm op, or "
        "every uploaded file — and is the sample count the percentiles rest on. "
        "`invalid` counts measurement phases that their guard rejected — a probe "
        "round, warmup or storm run whose server state at start or end was not "
        "the one the cell claims (the scan finished early, or was still running "
        "in an idle cell), or whose status counters could not be read — and "
        "those phases are not pooled; the recorded reasons are the cell's "
        "`phase_invalid_reasons` field in `baselines.tsv`. `failed` counts "
        "operations that did not return 200. A cell whose rejections exceed "
        "10% of its measured probe rounds — the only phase a guard can reject "
        "without failing the cell at runtime — or that recorded any failed "
        "operation is suspect: the renderer prints a WARNING to stderr, and "
        "the validity table reports the cell either way.",
        "",
    ]

    def guarded_background(profile):
        """Background whose phase guard the cell records."""
        if profile == scenarios.PROBE:
            return "scan"
        if scenarios.is_storm(profile):
            return scenarios.storm_parts(profile)[0]
        return scenarios.upload_background(profile)

    loads = [(scenarios.PROBE, scenarios.PROBE, scenarios.PROBE)]
    loads += [
        (
            scenarios.storm_label(kind, level),
            scenarios.storm_profile("idle", kind, level),
            scenarios.storm_profile("scan", kind, level),
        )
        for kind in scenarios.STORM_KINDS
        for level in scenarios.STORM_LEVELS
    ]
    loads.append((scenarios.UPLOAD_LOOP_LABEL, scenarios.IDLE_UPLOAD, scenarios.SCAN_UPLOAD))

    for images in images_list:
        lines += [f"### {images} images", ""]
        table = []
        for label, idle_profile, scan_profile in loads:
            row = [label]
            for background, profile in (
                ("idle", idle_profile),
                ("scan", scan_profile),
            ):
                cell = _cell_for(metadata, rows, images, profile)
                row.append(_accesses(rows, cell, background))
                if cell is not None and guarded_background(profile) == background:
                    row.append(_validity_value(metadata, cell, "phase_invalid"))
                    row.append(_validity_value(metadata, cell, "failed"))
                else:
                    row.extend([NA, NA])
            table.append(row)
        lines += _md_table(
            [
                "profile",
                "idle accesses",
                "idle invalid",
                "idle failed",
                "scan accesses",
                "scan invalid",
                "scan failed",
            ],
            ["left", "right", "right", "right", "right", "right", "right"],
            table,
        )
        lines.append("")

    lines += [
        "## Foreground latency",
        "",
        "One row per load profile: the access, its probe or storm client count, "
        "and its upload phase. The at-rest (`idle`) and during-scan (`scan`) "
        "measurements sit side by side; the difference between them is the "
        "indexing-contention cost.",
        "",
    ]
    def variant_rank(variant):
        if variant == "":
            return (0, 0)
        if variant.startswith("storm-"):
            _, kind, level = variant.split("-")
            return (
                1,
                scenarios.STORM_KINDS.index(kind) * len(scenarios.STORM_LEVELS)
                + scenarios.STORM_LEVELS.index(level),
            )
        return (2, 0)

    probe_accesses = scenarios.REPORT_ACCESSES
    storm_subjects = scenarios.STORM_ACCESS
    for images in images_list:
        table = []
        entries: dict = {}

        def record(label, variant, background, cell, subject, family="latency"):
            if cell is None:
                return
            values = (
                _p50_text(rows, cell, family, background, subject),
                _stat_text(rows, cell, family, background, subject, "p95"),
                _stat_text(rows, cell, family, background, subject, "max"),
            )
            if values == (NA, NA, NA):
                return
            key = (
                scenarios.ACCESS_ORDER.get(subject, 99),
                *variant_rank(variant),
                subject,
                variant,
            )
            slot = entries.setdefault(key, {"idle": (NA, NA, NA), "scan": (NA, NA, NA)})
            slot[background] = values

        _probe_profile, probe_cell = _probe_cell(groups, metadata, rows, images)
        for subject in probe_accesses:
            for background in ("idle", "scan"):
                record(subject, "", background, probe_cell, subject)
        for profile in groups[images]:
            if not scenarios.is_storm(profile):
                continue
            background, kind, level = _storm_parts(profile)
            subject = storm_subjects.get(kind)
            if subject is None:
                continue
            cell = _cell_for(metadata, rows, images, profile)
            record(subject, f"storm-{kind}-{level}", background, cell, subject)
        for profile, background in (
            (scenarios.IDLE_UPLOAD, "idle"),
            (scenarios.SCAN_UPLOAD, "scan"),
        ):
            cell = _cell_for(metadata, rows, images, profile)
            for phase in ("submit", "visible"):
                record(phase, "upload", background, cell, phase, family="upload")
        for key in sorted(entries):
            _order, _rank, _rank2, subject, variant = key
            if variant == "upload":
                name = scenarios.upload_label(subject)
            elif variant.startswith("storm-"):
                _, kind, level = variant.split("-")
                name = scenarios.storm_label(kind, level)
            else:
                name = scenarios.probe_label(subject)
            idle = entries[key]["idle"]
            scan = entries[key]["scan"]
            table.append([name, *idle, *scan])
        if not table:
            continue
        lines += [f"### {images} images", ""]
        lines += _md_table(
            [
                "profile",
                "idle p50 ms ± CI",
                "p95",
                "max",
                "scan p50 ms ± CI",
                "p95",
                "max",
            ],
            ["left", "left", "right", "right", "left", "right", "right"],
            table,
        )
        lines.append("")


    lines += [
        "## Scan time",
        "",
        "Wall clock of the populate scan and the resulting ingest rate for a few "
        "load profiles, one column per image count. The no-load row is the median "
        "populate scan of the idle-storm cells, which scan with no foreground "
        "traffic; the storm rows add the named client load; the upload row adds a "
        "serial upload loop (submit, then poll visibility). Each cell contributes "
        "one scan wall, so treat small differences between rows as noise.",
        "",
    ]

    def wall_of(images, profile):
        cell = _cell_for(metadata, rows, images, profile)
        wall_row = _lookup(rows, cell, "wall", "scan", "scan", "p50") if cell else None
        try:
            wall = float(wall_row["value"]) if wall_row else None
        except (TypeError, ValueError):
            wall = None
        return wall if wall and wall > 0 else None

    def cell_text(images, wall):
        return f"{wall:.1f} s ({images / wall:.1f} img/s)" if wall else NA

    load_profiles = (
        (
            scenarios.storm_label(kind, level),
            scenarios.storm_profile("scan", kind, level),
        )
        for kind in scenarios.STORM_KINDS
        for level in scenarios.STORM_LEVELS
    )
    table = []
    row = [scenarios.NO_LOAD_LABEL]
    for images in images_list:
        walls = [
            wall
            for profile in groups[images]
            if scenarios.is_storm(profile)
            and scenarios.storm_parts(profile)[0] == "idle"
            and (wall := wall_of(images, profile)) is not None
        ]
        row.append(cell_text(images, statistics.median(walls) if walls else None))
    table.append(row)
    for label, profile in (
        *load_profiles,
        (scenarios.UPLOAD_LOOP_LABEL, scenarios.SCAN_UPLOAD),
    ):
        row = [label]
        for images in images_list:
            row.append(cell_text(images, wall_of(images, profile)))
        table.append(row)
    lines += _md_table(
        ["load profile"] + [f"{images} images" for images in images_list],
        ["left"] + ["right"] * len(images_list),
        table,
    )
    lines.append("")

    lines += [
        "## Memory and storage footprint",
        "",
        "Peak process memory and store sizes at the end of each cell, one table per "
        "metric with profile rows and image-count columns, rows in a fixed "
        "profile order. All "
        "values are in MB. Footprint is expected to be broadly consistent across "
        "access types at the same image count; a large outlier marks a possible "
        "bottleneck or an optimization target, and growth with image count that "
        "outpaces the library size is likewise worth investigating.",
        "",
    ]
    memory_columns = (
        ("vm_hwm_mb", "peak VMM", 1048576.0 / 1e6),
        ("compressed_mb", "compressed", 1.0),
        ("index_v5_mb", "index DB", 1.0),
        ("temp_db_mb", "temp", 1.0),
        ("cache_db_mb", "cache", 1.0),
    )
    values_by_cell = {}
    for images in images_list:
        for profile in groups[images]:
            cell = _cell_for(metadata, rows, images, profile)
            values_by_cell[(images, profile)] = cell

    def metric_value(gauge, scale, images, profile):
        cell = values_by_cell.get((images, profile))
        value = _gauge(rows, cell, gauge) if cell else NA
        if value == NA:
            return NA
        try:
            return f"{float(value) * scale:.1f}"
        except (TypeError, ValueError):
            return NA

    # Rows in one fixed profile order across every metric table, so a diff
    # between recordings moves values, not rows.
    profiles_present = sorted({profile for images in images_list for profile in groups[images]})
    for gauge, label, scale in memory_columns:
        table = []
        for profile in profiles_present:
            row = [profile]
            row.extend(metric_value(gauge, scale, images, profile) for images in images_list)
            table.append(row)
        lines += _md_table(
            ["profile"] + [f"{label} ({images})" for images in images_list],
            ["left"] + ["right"] * len(images_list),
            table,
        )
        lines.append("")

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
