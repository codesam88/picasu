#!/usr/bin/env python3
"""Extract benchmark summary from one or more harness-A output dirs.

Usage:
    utils/bench/extract-bench.py /tmp/.../bench-after-N4-100 [...]

Reads harness-a.log, status-final.json and hyperfine.json from each
OUT dir (as produced by the bench workload) and prints one metric per
line, one column per cell: image count/bytes, scan wall time + sec/MB,
during-scan latency medians and worst rounds, post-scan hyperfine
medians, and per-storm-profile aggregates (median of run medians,
worst of run maxes).

Scan counters come from status-final.json and are validity-checked only:
a non-completed state, any failure, or a processed/matched mismatch prints
a WARN line to stderr instead of a table row. Timings come from the log's
`scan state=` line and the final `probe round=` line; hyperfine means from
hyperfine.json.
Exit non-zero if a dir is missing expected artifacts.
"""

import argparse
import json
import re
import statistics
import sys
from pathlib import Path


def parse_log(outdir: Path) -> dict:
    log = (outdir / "harness-a.log").read_text()
    row: dict = {"benchmark": outdir.name}

    m = re.search(r"(?:^|\s)N=(\d+)", log, re.M)
    if m:
        row["cpu"] = m.group(1)
    m = re.search(r"image_count=(\d+) image_bytes=(\d+)", log)
    if m:
        row["images"] = int(m.group(1))
        row["bytes"] = int(m.group(2))
    m = re.search(r"scan state=(\w+) polls=(\d+) wall_ms=(\d+)", log)
    if m:
        row["scan_state"] = m.group(1)
        row["polls"] = int(m.group(2))
        row["scan_wall_s"] = int(m.group(3)) / 1000.0
    # Repeated scans: median wall time wins over any single run.
    m = re.search(r"scan wall median_ms=([\d.]+) over (\d+) run\(s\): ([\d,]+)", log)
    if m:
        row["scan_wall_s"] = float(m.group(1)) / 1000.0
        row["scan_runs"] = len(m.group(3).split(","))
        row["scan_runs_ms"] = [float(x) / 1000.0 for x in m.group(3).split(",")]
    # Pool ALL fetch rounds (every round is an equally representative
    # random sample; reporting only the last one discards ~90% of the
    # measurements). Mean of round means + total fetch count.
    fetches = re.findall(
        r"probe round=\d+ rows=(\d+) state=\w+ fetch: thumb: n=(\d+) mean=([\d.]+)",
        log,
    )
    if fetches:
        row["probe_rows"] = max(int(f[0]) for f in fetches)
        row["probe_rounds"] = len(fetches)
        total_n = sum(int(f[1]) for f in fetches)
        row["scan_thumb_mean"] = (
            sum(int(f[1]) * float(f[2]) for f in fetches) / total_n if total_n else 0.0
        )
        row["scan_thumb_n"] = total_n
        row["scan_thumb_worst"] = max(float(f[2]) for f in fetches)
    else:
        rounds = re.findall(
            r"probe round=\d+ rows=(\d+) state=\w+ (?:seq: seq: n=\d+ ok=\d+ mean=([\d.]+).*?conc(?:-P\d+)?: n=\d+ ok=\d+ mean=([\d.]+)"
            r"|hyperfine: compressed-get mean=([\d.]+) s.*?prefetch mean=([\d.]+) s)",
            log,
        )
        if rounds:
            last = rounds[-1]
            row["probe_rows"] = int(last[0]) if last[0] else 0
            if last[1]:
                # Legacy curl-sweep format: full-list mean, no per-round
                # prefetch — prefetch stays absent (n/a), not zero.
                row["scan_thumb_mean"] = float(last[1])
            else:
                # Previous format: per-round compressed hyperfine mean.
                row["scan_thumb_mean"] = float(last[3])
    # Pool ALL per-round hyperfine lines the same way: median of round
    # medians (robust to single stalled runs and single stalled rounds).
    rh = re.findall(
        r"probe round=\d+ .*?hyperfine: prefetch mean=[\d.]+ s median=([\d.]+) s.*?edit-rating mean=[\d.]+ s median=([\d.]+) s",
        log,
    )
    if rh:
        row["scan_pref_median"] = statistics.median(float(r[0]) for r in rh)
        row["scan_edit_median"] = statistics.median(float(r[1]) for r in rh)
        row["scan_pref_worst"] = max(float(r[0]) for r in rh)
        row["scan_edit_worst"] = max(float(r[1]) for r in rh)
        row["scan_rounds"] = len(rh)
    else:
        # Previous format: per-round compressed+prefetch hyperfine means.
        rh_old = re.findall(
            r"probe round=\d+ .*?hyperfine: compressed-get mean=([\d.]+) s.*?prefetch mean=([\d.]+) s",
            log,
        )
        if rh_old:
            row["scan_thumb_mean"] = float(rh_old[-1][0])
            row["scan_pref_mean"] = float(rh_old[-1][1])
    # Post-scan hyperfine medians (idle server: mean ~= median, take median
    # for consistency with the during-scan columns).
    m = re.search(r"hyperfine compressed-get: mean=[\d.]+ s median=([\d.]+) s", log)
    if m:
        row["post_compressed"] = float(m.group(1))
    else:
        m = re.search(r"hyperfine compressed-get: mean=([\d.]+)", log)
        if m:
            row["post_compressed"] = float(m.group(1))
    m = re.search(r"hyperfine prefetch: mean=[\d.]+ s median=([\d.]+) s", log)
    if m:
        row["post_prefetch"] = float(m.group(1))
    else:
        m = re.search(r"hyperfine prefetch: mean=([\d.]+)", log)
        if m:
            row["post_prefetch"] = float(m.group(1))
    m = re.search(r"hyperfine edit-rating: mean=[\d.]+ s median=([\d.]+) s", log)
    if m:
        row["post_edit"] = float(m.group(1))
    else:
        m = re.search(r"hyperfine edit-rating: mean=([\d.]+)", log)
        if m:
            row["post_edit"] = float(m.group(1))
    # Storm summaries (one per scan run for storm-* workloads).
    storms = re.findall(
        r"^.*storm: profile=(\S+) ops=(\d+)/(\d+) median=([\d.]+)s "
        r"p95=([\d.]+)s max=([\d.]+)s failed=(\d+) wall=([\d.]+)s",
        log,
        re.M,
    )
    if storms:
        row["storms"] = [
            {
                "profile": s[0],
                "done": int(s[1]),
                "total": int(s[2]),
                "median": float(s[3]),
                "p95": float(s[4]),
                "max": float(s[5]),
                "failed": int(s[6]),
                "wall": float(s[7]),
            }
            for s in storms
        ]
    m = re.search(r"=== seeding images \(repo=(\S+) path=\S+\)", log)
    if not m:
        m = re.search(r"reused images copied from (\S+)", log)
    if m:
        row["image_repo"] = m.group(1)
    return row


def parse_status(outdir: Path) -> dict:
    try:
        return json.loads((outdir / "status-final.json").read_text())
    except (OSError, json.JSONDecodeError) as exc:
        print(f"error: {outdir}: cannot read status-final.json: {exc}", file=sys.stderr)
        sys.exit(1)


def _storm_summary(storms: list, profile: str) -> dict | None:
    """Aggregate per-run storm summaries: median of medians, worst of max."""
    runs = [s for s in storms if s["profile"] == profile]
    if not runs:
        return None
    return {
        "ops": f"{sum(s['done'] for s in runs)}/{sum(s['total'] for s in runs)}",
        "med": statistics.median(s["median"] for s in runs),
        "p95": max(s["p95"] for s in runs),
        "max": max(s["max"] for s in runs),
        "fail": sum(s["failed"] for s in runs),
    }


def _cell(row: dict, status: dict) -> dict:
    """Flatten one parsed cell into label-agnostic string/number fields."""
    mb = row.get("bytes", 0) / 1e6
    wall = row.get("scan_wall_s", 0.0)
    cell = {
        "name": row.get("benchmark", ""),
        "setup": (
            f"NCPU={row.get('cpu', '?')} IMAGES={row.get('images', '?')} "
            f"IMAGE_REPO={row.get('image_repo', '?')}"
        ),
        "state": status.get("state", "?"),
        "scan_time_s": round(wall),
        "s/MB": round(wall / mb, 1) if mb else "n/a",
        "MB": round(mb, 1),
        "scanned": status.get("scanned", "?"),
        "matched": status.get("matched", "?"),
        "processed": status.get("processed", "?"),
        "failed": status.get("failed", "?"),
        "scan_runs_s": ",".join(f"{w:.0f}" for w in row.get("scan_runs_ms", [])) or "n/a",
        "scan_fetch_ms": _ms(row.get("scan_pref_median", row.get("scan_pref_mean"))),
        "fetch_worst_ms": _ms(row.get("scan_pref_worst")),
        "scan_thumb_ms": _ms(row.get("scan_thumb_mean")),
        "thumb_worst_ms": _ms(row.get("scan_thumb_worst")),
        "scan_edit_ms": _ms(row.get("scan_edit_median", row.get("scan_edit_mean"))),
        "edit_worst_ms": _ms(row.get("scan_edit_worst")),
        "post_thumb_ms": _ms(row.get("post_compressed")),
        "post_fetch_ms": _ms(row.get("post_prefetch")),
        "post_edit_ms": _ms(row.get("post_edit")),
    }
    for profile in ("storm-diff", "storm-read", "storm-idle"):
        summary = _storm_summary(row.get("storms", []), profile)
        if summary is None:
            cell[f"{profile}_ops"] = "n/a"
            cell[f"{profile}_med_ms"] = "n/a"
            cell[f"{profile}_p95_ms"] = "n/a"
            cell[f"{profile}_max_ms"] = "n/a"
            cell[f"{profile}_fail"] = "n/a"
        else:
            cell[f"{profile}_ops"] = summary["ops"]
            cell[f"{profile}_med_ms"] = round(summary["med"] * 1000, 1)
            cell[f"{profile}_p95_ms"] = round(summary["p95"] * 1000, 1)
            cell[f"{profile}_max_ms"] = round(summary["max"] * 1000, 1)
            cell[f"{profile}_fail"] = summary["fail"]
    return cell


def _ms(value) -> object:
    return round(value * 1000, 1) if value is not None else "n/a"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("outdirs", nargs="+", help="harness-A OUT dirs")
    args = parser.parse_args()

    cells = []
    for raw in args.outdirs:
        outdir = Path(raw)
        if not (outdir / "harness-a.log").exists():
            print(f"error: {outdir}: no harness-a.log", file=sys.stderr)
            sys.exit(1)
        cells.append(_cell(parse_log(outdir), parse_status(outdir)))

    profiles = [
        p
        for p in ("storm-diff", "storm-read", "storm-idle")
        if any(c[f"{p}_ops"] != "n/a" for c in cells)
    ]
    for i, cell in enumerate(cells, 1):
        print(f"[{i}] name: {cell['name']}")
        print(f"    {cell['setup']}")
    labels = [
        "scan_time_s",
        "s/MB",
        "MB",
        "scan_runs_s",
        "scan_fetch_ms",
        "fetch_worst_ms",
        "scan_thumb_ms",
        "thumb_worst_ms",
        "scan_edit_ms",
        "edit_worst_ms",
        "post_thumb_ms",
        "post_fetch_ms",
        "post_edit_ms",
    ] + [f"{p}_{c}" for p in profiles for c in ("ops", "med_ms", "p95_ms", "max_ms", "fail")]
    width = max([len(label) for label in labels] + [8])
    col_widths = []
    for i in range(len(cells)):
        widest = max([len(str(cells[i][label])) for label in labels] + [len(f"[{i + 1}]"), 10])
        col_widths.append(widest)
    print(
        f"{'metric':<{width}} "
        + " ".join(f"{f'[{i}]':>{w}}" for i, w in zip(range(1, len(cells) + 1), col_widths))
    )
    for label in labels:
        print(
            f"{label:<{width}} "
            + " ".join(f"{str(cell[label]):>{w}}" for cell, w in zip(cells, col_widths))
        )
    for cell in cells:
        if cell["state"] != "completed":
            print(
                f"WARN {cell['name']}: state={cell['state']} (expected completed)",
                file=sys.stderr,
            )
        if isinstance(cell["failed"], int) and cell["failed"] > 0:
            print(f"WARN {cell['name']}: failed={cell['failed']}", file=sys.stderr)
        if (
            isinstance(cell["processed"], int)
            and isinstance(cell["matched"], int)
            and cell["processed"] != cell["matched"]
        ):
            print(
                f"WARN {cell['name']}: processed={cell['processed']} matched={cell['matched']}",
                file=sys.stderr,
            )
    print("units: scan_time s; s/MB s per MB; all latency columns ms.")
    print("median/worst latency: pre[fetch], [thumb]nail, metadata[edit], scan and post")


if __name__ == "__main__":
    main()
