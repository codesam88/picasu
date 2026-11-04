#!/usr/bin/env python3
"""Extract benchmark summary from one or more harness-A output dirs.

Usage:
    utils/bench/extract-bench.py /tmp/.../bench-after-N4-100 [...]

Reads harness-a.log, status-final.json and hyperfine.json from each
OUT dir (as produced by utils/bench/harness-a.sh) and prints one
row per cell: image count/bytes, scan wall time + sec/MB, last-round
probe means (sequential + concurrent), and post-scan hyperfine means.

Scan counters come from status-final.json (scanned/matched/processed/
failed); timings from the log's `scan state=` line and the final
`probe round=` line; hyperfine means from hyperfine.json.
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
    return row


def parse_status(outdir: Path) -> dict:
    try:
        return json.loads((outdir / "status-final.json").read_text())
    except (OSError, json.JSONDecodeError) as exc:
        print(f"error: {outdir}: cannot read status-final.json: {exc}", file=sys.stderr)
        sys.exit(1)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("outdirs", nargs="+", help="harness-A OUT dirs")
    args = parser.parse_args()

    header = (
        f"{'benchmark':<26}{'cpu':>4}{'images':>7}{'MB':>9}"
        f"{'scan_time':>10}{'s/MB':>8}{'state':>11}"
        f"{'scan_fetch':>13}{'scan_thumb':>14}{'scan_edit':>13}"
        f"{'post_thumb':>14}{'post_fetch':>13}{'post_edit':>13}"
    )
    print(header)

    def ms(row: dict, *keys: str, width: int) -> str:
        for key in keys:
            val = row.get(key)
            if val is not None:
                return f"{val * 1000:>{width}.1f}"
        return "n/a".rjust(width)

    for raw in args.outdirs:
        outdir = Path(raw)
        if not (outdir / "harness-a.log").exists():
            print(f"error: {outdir}: no harness-a.log", file=sys.stderr)
            sys.exit(1)
        row = parse_log(outdir)
        status = parse_status(outdir)
        mb = row.get("bytes", 0) / 1e6
        wall = row.get("scan_wall_s", 0.0)
        sec_per_mb = (wall / mb) if mb else 0.0
        print(
            f"{row.get('benchmark', ''):<26}"
            f"{row.get('cpu', '?'):>4}"
            f"{row.get('images', 0):>7}"
            f"{mb:>9.1f}"
            f"{wall:>10.0f}"
            f"{sec_per_mb:>8.1f}"
            f"{status.get('state', '?'):>11}"
            f"{ms(row, 'scan_pref_median', 'scan_pref_mean', width=13)}"
            f"{ms(row, 'scan_thumb_mean', width=14)}"
            f"{ms(row, 'scan_edit_median', 'scan_edit_mean', width=13)}"
            f"{ms(row, 'post_compressed', width=14)}"
            f"{ms(row, 'post_prefetch', width=13)}"
            f"{ms(row, 'post_edit', width=13)}"
        )
        counts = (
            f"  scanned={status.get('scanned')} matched={status.get('matched')} "
            f"processed={status.get('processed')} failed={status.get('failed')} "
            f"probe_rows={row.get('probe_rows', '?')}"
        )
        if "scan_rounds" in row:
            counts += f" rounds={row['scan_rounds']}"
        if "scan_thumb_n" in row:
            counts += f" thumb_n={row['scan_thumb_n']}"
        worst = []
        for key, label in (
            ("scan_pref_worst", "fetch"),
            ("scan_thumb_worst", "thumb"),
            ("scan_edit_worst", "edit"),
        ):
            if key in row:
                worst.append(f"{label}={row[key] * 1000:.1f}")
        if worst:
            counts += f" worst_ms=[{', '.join(worst)}]"
        if "scan_runs_ms" in row:
            runs = ",".join(f"{w:.0f}" for w in row["scan_runs_ms"])
            counts += f" scan_runs_s=[{runs}]"
        print(counts)
    print(
        "units: scan_time s; s/MB s per MB; all latency columns ms (hyperfine-derived columns report medians-of-round-medians, robust to stalls; scan_thumb pools all rounds' random fetches; worst_ms lists each column's worst round)"
    )


if __name__ == "__main__":
    main()
