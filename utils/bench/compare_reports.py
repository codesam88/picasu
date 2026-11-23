#!/usr/bin/env python3
"""Compare long-format report.tsv or baselines.tsv files."""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import battery  # noqa: E402

NA = battery.NA
REPORT_COLUMNS = list(battery.REPORT_COLUMNS)
BASELINE_COLUMNS = list(battery.BASELINE_COLUMNS)


def _clients(value):
    text = str(value).strip() if value is not None else ""
    return int(text) if text.isdigit() else value


def _is_number(value) -> bool:
    if value is None:
        return False
    text = str(value).strip()
    if text in ("", "n/a", "NA", NA):
        return False
    try:
        float(text)
    except TypeError, ValueError:
        return False
    return True


def _num(value):
    return float(str(value).strip())


def _scalar_value(value):
    if value is None:
        return None
    text_ = str(value).strip()
    if text_ in ("", "n/a", "NA", NA):
        return None
    try:
        return float(text_)
    except TypeError, ValueError:
        return None


_ABS_FLOORS = {"ms": 1.0, "us": 1000.0, "s": 0.1, "MB": 1.0, "ops": 1.0}


def tolerance_for(unit, baseline, relative: float = 0.5) -> float:
    """Tolerance: relative band of the baseline, floored by the metric's unit."""
    base = 0.0
    if baseline is not None:
        try:
            base = abs(float(baseline))
        except TypeError, ValueError:
            base = 0.0
    return max(_ABS_FLOORS.get(str(unit), 1.0), relative * base)


def _fmt_scalar(value):
    if value is None:
        return "n/a"
    if isinstance(value, float):
        if math.isnan(value) or math.isinf(value):
            return "n/a"
        if value.is_integer():
            return str(int(value))
        return f"{value:g}"
    return str(value)


def _read_long(path: Path):
    """Return (metadata dict, list of row dicts) for a long TSV."""
    path = Path(path)
    lines = [line for line in path.read_text().splitlines() if line.strip()]
    if not lines:
        raise ValueError(f"{path}: empty report")
    header = None
    header_index = None
    for index, line in enumerate(lines):
        parts = line.split("\t")
        if parts and parts[0] == "cell" and len(parts) >= len(REPORT_COLUMNS):
            header = parts
            header_index = index
            break
    if header is None:
        raise ValueError(f"{path}: missing header")
    if (
        header[: len(REPORT_COLUMNS)] != REPORT_COLUMNS
        and header[: len(BASELINE_COLUMNS)] != BASELINE_COLUMNS
    ):
        # Accept either exact report or baseline header.
        if header != REPORT_COLUMNS and header != BASELINE_COLUMNS:
            raise ValueError(f"{path}: unexpected header")
    metadata = {}
    rows = []
    current_meta = {"identity": {}, "validity": {}}
    has_cell_meta = False
    for line in lines[header_index + 1 :]:
        if line.startswith("# cell: "):
            has_cell_meta = True
            # flush previous rows? rows already appended; metadata keyed by next cell
            try:
                payload = json.loads(line[len("# cell: ") :])
            except Exception:
                payload = {}
            current_meta = {
                "identity": payload.get("identity") or {},
                "validity": payload.get("validity") or {},
            }
            continue
        parts = line.split("\t")
        if len(parts) != len(header):
            continue
        row = dict(zip(header, parts, strict=True))
        cell = row.get("cell")
        # attach metadata if present
        if has_cell_meta:
            metadata.setdefault(cell, current_meta)
        rows.append(row)
    # If metadata was captured after rows? Re-associate by walking again is complex;
    # the above associates current_meta at row time, which is correct for # cell before rows.
    if not has_cell_meta:
        metadata = {}
    return metadata, rows


def _row_key(row):
    return (
        str(row.get("cell", "")),
        str(row.get("family", "")),
        str(row.get("background", "")),
        str(row.get("subject", "")),
        str(row.get("stat", "")),
    )


def _value_of(row):
    # baselines use 'value'; reports use 'value'. Both same.
    return row.get("value", row.get("baseline"))


def compare(baseline_path, new_path, tolerance=0.5):
    meta_left, rows_left = _read_long(Path(baseline_path))
    meta_right, rows_right = _read_long(Path(new_path))

    # Identity guard.
    refused = set()
    skipped = set()
    if meta_left and meta_right:
        for cell in set(meta_left) & set(meta_right):
            li = meta_left.get(cell, {}).get("identity") or {}
            ri = meta_right.get(cell, {}).get("identity") or {}
            if li.get("level") == "cell" or ri.get("level") == "cell":
                skipped.add(cell)
                continue
            for field in ("profile", "images"):
                if (
                    li.get(field) is not None
                    and ri.get(field) is not None
                    and li.get(field) != ri.get(field)
                ):
                    refused.add(cell)
                    print(
                        f"refused cell {cell}: identity mismatch on {field} "
                        f"({li.get(field)} vs {ri.get(field)})",
                        file=sys.stderr,
                    )
                    break

    def allowed(cell):
        return cell not in refused and cell not in skipped

    left_keys = {_row_key(r) for r in rows_left if allowed(r.get("cell"))}
    right_keys = {_row_key(r) for r in rows_right if allowed(r.get("cell"))}

    left_map = {}
    for r in rows_left:
        if not allowed(r.get("cell")):
            continue
        left_map[_row_key(r)] = r
    right_map = {}
    for r in rows_right:
        if not allowed(r.get("cell")):
            continue
        right_map[_row_key(r)] = r

    shared = left_keys & right_keys
    missing_left = right_keys - left_keys  # in new, not baseline
    missing_right = left_keys - right_keys  # in baseline, not new

    print(
        f"compared={len(shared)} missing_on_baseline={len(missing_right)} "
        f"missing_on_new={len(missing_left)} refused={len(refused)}",
        file=sys.stderr,
    )

    rows_out = []
    counts = {}
    all_keys = sorted(left_keys | right_keys)
    for key in all_keys:
        cell, family, background, subject, stat = key
        left_row = left_map.get(key)
        right_row = right_map.get(key)
        has_left = left_row is not None
        has_right = right_row is not None
        base_val = _scalar_value(_value_of(left_row)) if has_left else None
        cur_val = _scalar_value(_value_of(right_row)) if has_right else None

        if key in missing_right:
            flag = "missing"
        elif key in missing_left:
            flag = "new"
        elif base_val is None or cur_val is None:
            flag = "n/a"
        else:
            unit = (left_row or {}).get("unit") or (right_row or {}).get("unit")
            tol = tolerance_for(unit, base_val, tolerance)
            delta = cur_val - base_val
            dot = abs(delta) / tol if tol else float("inf")
            if dot > 1:
                flag = "moved"
            else:
                flag = "ok"

        if base_val is not None and cur_val is not None:
            delta = cur_val - base_val
            delta_pct = (delta / base_val * 100.0) if base_val != 0 else float("inf")
            unit = (left_row or {}).get("unit") or (right_row or {}).get("unit")
            tol = tolerance_for(unit, base_val, tolerance)
            dot = round(abs(delta) / tol, 3) if tol else None
        else:
            delta = None
            delta_pct = None
            unit = (left_row or {}).get("unit") or (right_row or {}).get("unit")
            tol = tolerance_for(unit, base_val, tolerance) if base_val is not None else None
            dot = None

        row = {
            "cell": cell,
            "family": family,
            "background": background,
            "subject": subject,
            "stat": stat,
            "metric": f"{family} {background} {subject} {stat}",
            "baseline": _fmt_scalar(base_val),
            "current": _fmt_scalar(cur_val),
            "delta": _fmt_scalar(delta),
            "delta_pct": _fmt_scalar(delta_pct),
            "tolerance": _fmt_scalar(tol),
            "delta_over_tolerance": _fmt_scalar(dot),
            "flag": flag,
        }
        rows_out.append(row)
        counts[flag] = counts.get(flag, 0) + 1

    return rows_out, counts


def _write_tsv(path, rows):
    columns = [
        "cell",
        "family",
        "background",
        "subject",
        "stat",
        "baseline",
        "current",
        "delta",
        "delta_pct",
        "tolerance",
        "delta_over_tolerance",
        "flag",
    ]
    lines = ["\t".join(columns)]
    for row in rows:
        lines.append("\t".join(str(row[col]) for col in columns))
    text = "\n".join(lines) + "\n"
    if path is None:
        sys.stdout.write(text)
    else:
        Path(path).write_text(text, encoding="utf-8")
    return text


def _render_markdown(rows, counts):
    lines = ["# Comparison report", ""]
    lines.append("## Flags")
    lines.append("")
    lines.append("| flag | count |")
    lines.append("| --- | --- |")
    for flag in ("missing", "new", "n/a", "moved", "ok"):
        lines.append(f"| {flag} | {counts.get(flag, 0)} |")
    lines.append("")
    interesting = [r for r in rows if r["flag"] in ("moved",)]
    lines.append("## Moved")
    lines.append("")
    if interesting:
        for row in interesting[:20]:
            lines.append(f"- {row['cell']} {row['metric']}: {row['delta']}")
    else:
        lines.append("- none")
    lines.append("")
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", help="baseline report.tsv or long-format baselines.tsv")
    parser.add_argument("new", help="new report.tsv")
    parser.add_argument("--tolerance", type=float, default=0.5, help="relative tolerance factor")
    parser.add_argument("--md", help="write Markdown summary")
    parser.add_argument("--tsv", help="write TSV output (default stdout)")
    parser.add_argument("--strict", action="store_true", help="exit 1 on flagged rows")
    args = parser.parse_args(argv)

    rows, counts = compare(
        Path(args.baseline),
        Path(args.new),
        tolerance=args.tolerance,
    )
    _write_tsv(Path(args.tsv) if args.tsv else None, rows)
    if args.md:
        Path(args.md).write_text(_render_markdown(rows, counts), encoding="utf-8")
    print(
        f"rows={len(rows)} "
        + " ".join(f"{k}={counts.get(k, 0)}" for k in ("missing", "new", "n/a", "moved", "ok"))
        + f" output={args.tsv if args.tsv else 'stdout'}",
        file=sys.stderr,
    )
    if args.strict and any(row["flag"] in ("missing", "n/a", "moved") for row in rows):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
