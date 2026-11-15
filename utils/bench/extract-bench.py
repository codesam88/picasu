#!/usr/bin/env python3
"""Extract benchmark summary from one or more bench output dirs.

Usage:
    utils/bench/extract-bench.py /tmp/.../bench-after-N4-100 [...]

Reads events.jsonl, harness-a.log, status-final.json and hyperfine.json
from each OUT dir (as produced by the bench workload) and prints one
metric per line, one column per cell: image count/bytes, scan wall time +
sec/MB, during-scan latency medians and worst rounds, post-scan hyperfine
medians, and per-storm-profile aggregates (median of run medians, worst
of run maxes).

Measurements prefer the cell's structured `events.jsonl` stream (see
:func:`read_events`); the transcript regexes below are the fallback for
cells recorded before the event stream existed, and for cells whose
stream is absent, truncated or written by a newer schema. Both paths
report the numbers the transcript records for a field, so a cell
extracted from events reports the same values as the same run extracted
from its log.

Scan counters come from status-final.json and are validity-checked only:
a non-completed state, any failure, or a processed/matched mismatch prints
a WARN line to stderr instead of a table row. Server stage costs come from
the `observe drain:` tables in server.log (see :func:`parse_drain_tables`).
Exit non-zero if a dir is missing expected artifacts.
"""

import argparse
import dataclasses
import json
import re
import statistics
import sys
from pathlib import Path

STAGE_NAMES = [
    "filter_snapshot_clone",
    "filter_par_iter",
    "compute_locate",
    "tree_refresh_total",
    "tree_refresh_build",
    "tree_refresh_sort",
    "tree_refresh_write_wait",
    "cpu_permit_wait",
    "cpu_exec",
    "prefetch_total",
    "blocking_queue_wait",
    "interactive_exec",
]


def _drain_scan_index(context: str, ordinal: int) -> int:
    for pattern in (r"scan job=(\d+)", r"\bjob=(\d+)", r"scan=(\d+)", r"run=(\d+)"):
        m = re.search(pattern, context)
        if m:
            return int(m.group(1))
    return ordinal


def parse_drain_tables(log) -> list:
    """Parse ``observe drain:`` tables from a server log.

    Accepts either log text or a path-like object. Returns a list of
    per-scan tables: ``{"scan": int, "context": str, "stages": [row, ...]}``.
    Each stage row carries ``stage, n, p50_us, p95_us, max_us``.
    """
    if isinstance(log, (str, bytes)):
        text = log.decode() if isinstance(log, bytes) else log
    else:
        try:
            text = Path(log).read_text()
        except OSError, TypeError:
            text = str(log)
    tables: list = []
    lines = text.splitlines()
    i = 0
    ordinal = 0
    while i < len(lines):
        if "observe drain:" not in lines[i]:
            i += 1
            continue
        context = lines[i].split("observe drain:", 1)[1].strip()
        ordinal += 1
        scan = _drain_scan_index(context, ordinal)
        stages: list = []
        j = i + 1
        while j < len(lines):
            stripped = lines[j].strip()
            if not stripped:
                break
            if stripped.startswith("stage"):
                j += 1
                continue
            m = re.match(r"^\s*(\S+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s*$", lines[j])
            if not m:
                break
            stage_name = m.group(1)
            if stage_name == "stage":
                j += 1
                continue
            stages.append(
                {
                    "stage": stage_name,
                    "n": int(m.group(2)),
                    "p50_us": int(m.group(3)),
                    "p95_us": int(m.group(4)),
                    "max_us": int(m.group(5)),
                }
            )
            j += 1
        if stages:
            tables.append({"scan": scan, "context": context, "stages": stages})
        i = j if j > i else i + 1
    return tables


def parse_drain_rows(log) -> list:
    """Flatten :func:`parse_drain_tables` into per-stage rows.

    Each row carries ``scan, scan_index, run, run_index, stage, n, p50_us,
    p95_us, max_us, context``.
    """
    rows: list = []
    for table in parse_drain_tables(log):
        scan = table["scan"]
        for stage in table["stages"]:
            rows.append(
                {
                    "scan": scan,
                    "scan_index": scan,
                    "run": scan,
                    "run_index": scan - 1,
                    "stage": stage["stage"],
                    "n": stage["n"],
                    "p50_us": stage["p50_us"],
                    "p95_us": stage["p95_us"],
                    "max_us": stage["max_us"],
                    "context": table["context"],
                }
            )
    return rows


parse_drain_table = parse_drain_tables
parse_drains = parse_drain_tables
parse_observer_drain = parse_drain_tables
parse_observer_drains = parse_drain_tables
extract_drain_tables = parse_drain_tables
parse_drain = parse_drain_rows
parse_drain_table_rows = parse_drain_rows
parse_stage_rows = parse_drain_rows
parse_drain_stages = parse_drain_rows


# --- structured events -------------------------------------------------------
EVENTS_FILE = "events.jsonl"
EVENT_SCHEMA_VERSION = 1

# v1 event types: Events attribute, required keys, numeric keys. A numeric
# key may be null (the producer emits it when its source value is absent),
# but a required numeric key must carry a number.
_V1_EVENTS = {
    "probe_round": (
        "probe_rounds",
        ("round", "rows"),
        ("round", "rows", "fetch_n", "fetch_mean", "prefetch_median", "edit_median"),
    ),
    "storm_sample": (
        "storm_samples",
        ("profile", "client", "op", "latency_ms", "code"),
        ("client", "op", "latency_ms"),
    ),
    "storm_summary": (
        "storm_summaries",
        ("profile", "done", "total", "median", "p95", "max", "failed", "wall"),
        ("done", "total", "median", "p95", "max", "failed", "wall"),
    ),
    "upload_file": (
        "upload_files",
        ("file", "submit_ms", "visible_ms"),
        ("submit_ms", "visible_ms"),
    ),
    "upload_summary": (
        "upload_summaries",
        ("submit_p50", "visible_p50"),
        ("submit_p50", "visible_p50"),
    ),
    "scan_state": ("scan_states", ("run", "state", "wall_ms"), ("run", "wall_ms")),
    "verify_result": (
        "verify_results",
        ("kind", "matched", "total"),
        ("matched", "total"),
    ),
}


class EventsError(Exception):
    """`events.jsonl` holds no complete, supported event stream."""


@dataclasses.dataclass
class Events:
    """Typed v1 event stream: one list per event type, in emission order."""

    probe_rounds: list = dataclasses.field(default_factory=list)
    storm_samples: list = dataclasses.field(default_factory=list)
    storm_summaries: list = dataclasses.field(default_factory=list)
    upload_files: list = dataclasses.field(default_factory=list)
    upload_summaries: list = dataclasses.field(default_factory=list)
    scan_states: list = dataclasses.field(default_factory=list)
    verify_results: list = dataclasses.field(default_factory=list)


def read_events(outdir) -> Events:
    """Read ``events.jsonl`` from a cell OUT dir into typed v1 event lists.

    ``FileNotFoundError`` means the cell carries no event stream (recorded
    before the stream existed). ``EventsError`` means the file is not a
    complete v1 stream: an unknown schema version, a partial line from a
    producer that died mid-write, an unknown type or a missing key.
    """
    path = Path(outdir) / EVENTS_FILE
    return parse_events(path.read_text(), source=str(path))


def parse_events(text: str, *, source: str = EVENTS_FILE) -> Events:
    """Parse one v1 event stream; see :func:`read_events` for the errors."""
    events = Events()
    if not text:
        raise EventsError(f"{source}: empty event stream")
    if not text.endswith("\n"):
        # The producer appends whole lines and flushes each one, so a
        # missing final newline is a line cut in half.
        raise EventsError(f"{source}: truncated event stream (partial last line)")
    for lineno, line in enumerate(text.splitlines(), 1):
        where = f"{source}:{lineno}"
        try:
            event = json.loads(line)
        except json.JSONDecodeError as exc:
            raise EventsError(f"{where}: malformed JSON ({exc.msg})") from None
        if not isinstance(event, dict):
            raise EventsError(f"{where}: event is not a JSON object")
        version = event.get("v")
        if not isinstance(version, int) or isinstance(version, bool):
            raise EventsError(f"{where}: event carries no integer schema version 'v'")
        if version != EVENT_SCHEMA_VERSION:
            raise EventsError(
                f"{where}: event schema version v={version} is not supported "
                f"(this reader implements v={EVENT_SCHEMA_VERSION})"
            )
        kind = event.get("type")
        spec = _V1_EVENTS.get(kind)
        if spec is None:
            raise EventsError(f"{where}: unknown v{version} event type {kind!r}")
        attribute, required, numeric = spec
        missing = [key for key in required if key not in event]
        if missing:
            raise EventsError(f"{where}: {kind} event missing {', '.join(missing)}")
        for key in numeric:
            value = event.get(key)
            if value is None:
                if key in required:
                    raise EventsError(f"{where}: {kind} event has no numeric {key!r}")
                continue
            if isinstance(value, bool) or not isinstance(value, (int, float)):
                raise EventsError(f"{where}: {kind} event has non-numeric {key!r}")
        getattr(events, attribute).append(event)
    return events


# Transcript anchors used to decide whether an event stream is complete: a
# stream that lags its transcript missed a write (a cell killed mid-run, or
# an extract taken while the workload is still running), so its counts fall
# short. The transcript is the record of what the run wrote.
_TRANSCRIPT_FETCH_RE = re.compile(r"probe round=\d+ rows=\d+ state=\w+ fetch: thumb: n=(\d+) mean=")
_TRANSCRIPT_HYPERFINE_RE = re.compile(
    r"probe round=\d+ .*?hyperfine: prefetch mean=[\d.]+ s median=([\d.]+) s"
    r".*?edit-rating mean=[\d.]+ s median=([\d.]+) s"
)
_TRANSCRIPT_SCAN_RE = re.compile(r"scan run \d+/\d+: state=\w+ polls=\d+ wall_ms=\d+")
_TRANSCRIPT_SCAN_MEDIAN_RE = re.compile(
    r"scan wall median_ms=([\d.]+) over (\d+) run\(s\): ([\d,]+)"
)
_TRANSCRIPT_STORM_RE = re.compile(
    r"^.*storm: profile=(\S+) ops=(\d+)/(\d+) median=([\d.]+)s "
    r"p95=([\d.]+)s max=([\d.]+)s failed=(\d+) wall=([\d.]+)s(?: kind=(\S+))?",
    re.M,
)
_TRANSCRIPT_VERIFY_RE = re.compile(r"^.*storm verify: matched (\d+)/(\d+) canary writes", re.M)


def fetch_rounds(events: Events) -> list:
    """Probe rounds that sampled fetches; a round without samples has no mean."""
    return [
        event
        for event in events.probe_rounds
        if event.get("fetch_n") and isinstance(event.get("fetch_mean"), (int, float))
    ]


def hyperfine_rounds(events: Events) -> list:
    """Probe rounds whose hyperfine summary carried both medians."""
    return [
        event
        for event in events.probe_rounds
        if isinstance(event.get("prefetch_median"), (int, float))
        and isinstance(event.get("edit_median"), (int, float))
    ]


def storm_verifies(events: Events) -> list:
    """`(matched, total)` per storm canary verification."""
    return [
        (int(event["matched"]), int(event["total"]))
        for event in events.verify_results
        if event["kind"] == "storm"
    ]


def events_complete(events: Events, log: str) -> tuple:
    """Check the stream against the transcript; ``(ok, reason)``."""
    checks = (
        ("probe rounds", len(fetch_rounds(events)), len(_TRANSCRIPT_FETCH_RE.findall(log))),
        (
            "hyperfine rounds",
            len(hyperfine_rounds(events)),
            len(_TRANSCRIPT_HYPERFINE_RE.findall(log)),
        ),
        ("scan runs", len(events.scan_states), len(_TRANSCRIPT_SCAN_RE.findall(log))),
        ("storm summaries", len(events.storm_summaries), len(_TRANSCRIPT_STORM_RE.findall(log))),
        ("storm verifies", len(storm_verifies(events)), len(_TRANSCRIPT_VERIFY_RE.findall(log))),
    )
    for name, have, want in checks:
        if have != want:
            return False, f"{name}: {have} event(s) for {want} transcript line(s)"
    return True, ""


def read_cell_events(outdir: Path, log: str):
    """Return the cell's complete event stream, or None to use the log.

    An unusable stream never fails the extraction: it warns and falls back,
    because the transcript carries the same measurements.
    """
    path = outdir / EVENTS_FILE
    try:
        events = read_events(outdir)
    except FileNotFoundError:
        return None
    except (OSError, UnicodeDecodeError) as exc:
        print(f"warn: {path}: {exc}; falling back to log lines", file=sys.stderr)
        return None
    except EventsError as exc:
        print(f"warn: {exc}; falling back to log lines", file=sys.stderr)
        return None
    complete, reason = events_complete(events, log)
    if not complete:
        print(f"warn: {path}: incomplete ({reason}); falling back to log lines", file=sys.stderr)
        return None
    return events


# The event stream carries full-precision floats. These two helpers pin a
# value to the precision the transcript records, which is also the
# precision every recorded cell and checked-in baseline was produced with,
# so a cell extracted from events reports the same number as the same run
# extracted from its log.
def _recorded(value: float, digits: int) -> float:
    """Quantise to a decimal rendering (seconds)."""
    return float(f"{value:.{digits}f}")


def _recorded_median(value: float) -> float:
    """Quantise to the transcript's ``:g`` rendering (scan-wall median)."""
    return float(f"{value:g}")


def _storm_from_event(event: dict) -> dict:
    profile = event["profile"]
    kind = event.get("kind") or None
    key = profile if not kind or profile.endswith(kind) else f"{profile}-{kind}"
    return {
        "key": key,
        "done": int(event["done"]),
        "total": int(event["total"]),
        "median": _recorded(event["median"], 3),
        "p95": _recorded(event["p95"], 3),
        "max": _recorded(event["max"], 3),
        "failed": int(event["failed"]),
        "wall": _recorded(event["wall"], 1),
    }


def _apply_events(row: dict, events: Events, log: str) -> None:
    """Fill the measurement fields of ``row`` from the typed event stream."""
    rounds = fetch_rounds(events)
    if rounds:
        row["probe_rows"] = max(int(event["rows"]) for event in rounds)
        row["probe_rounds"] = len(rounds)
        thumb = [_recorded(event["fetch_mean"], 4) for event in rounds]
        total_n = sum(int(event["fetch_n"]) for event in rounds)
        row["scan_thumb_mean"] = (
            sum(int(event["fetch_n"]) * mean for event, mean in zip(rounds, thumb)) / total_n
        )
        row["scan_thumb_n"] = total_n
        row["scan_thumb_worst"] = max(thumb)
    rounds = hyperfine_rounds(events)
    if rounds:
        prefetch = [event["prefetch_median"] for event in rounds]
        edit = [event["edit_median"] for event in rounds]
        row["scan_pref_median"] = statistics.median(prefetch)
        row["scan_edit_median"] = statistics.median(edit)
        row["scan_pref_worst"] = max(prefetch)
        row["scan_edit_worst"] = max(edit)
        row["scan_rounds"] = len(rounds)
    if events.scan_states:
        walls = [int(event["wall_ms"]) for event in events.scan_states]
        median = statistics.median(walls)
        # The transcript renders this median with `:g` when the run reached
        # its summary; a cell that died earlier falls back to the raw
        # scan-walls median, which keeps full precision.
        if _TRANSCRIPT_SCAN_MEDIAN_RE.search(log):
            median = _recorded_median(median)
        row["scan_wall_s"] = median / 1000.0
        row["scan_runs"] = len(walls)
        row["scan_runs_ms"] = [wall / 1000.0 for wall in walls]
    if events.storm_summaries:
        row["storms"] = [_storm_from_event(event) for event in events.storm_summaries]
    verifies = storm_verifies(events)
    if verifies:
        row["storm_verify"] = verifies


# --- cell parsing ------------------------------------------------------------
def _apply_setup(row: dict, log: str) -> None:
    """Cell identity: CPU count, image counts and the repo they came from.

    The event stream carries measurements only, so these stay transcript
    fields for every cell.
    """
    m = re.search(r"(?:^|\s)N=(\d+)", log, re.M)
    if m:
        row["cpu"] = m.group(1)
    m = re.search(r"image_count=(\d+) image_bytes=(\d+)", log)
    if m:
        row["images"] = int(m.group(1))
        row["bytes"] = int(m.group(2))
    m = re.search(r"=== seeding images \(repo=(\S+) path=\S+\)", log)
    if not m:
        m = re.search(r"reused images copied from (\S+)", log)
    if m:
        row["image_repo"] = m.group(1)


def _apply_log_measurements(row: dict, outdir: Path, log: str) -> None:
    """Measurement fields from transcript lines.

    Deprecated: this is the pre-event-stream dialect, kept for cells
    recorded without `events.jsonl` (old runs, and stats-only cells) and
    for streams that are unusable. Current cells take these fields from
    :func:`_apply_events`.
    """
    m = re.search(r"scan state=(\w+) polls=(\d+) wall_ms=(\d+)", log)
    if m:
        row["scan_state"] = m.group(1)
        row["polls"] = int(m.group(2))
        row["scan_wall_s"] = int(m.group(3)) / 1000.0
    # Repeated scans: median wall time wins over any single run.
    m = _TRANSCRIPT_SCAN_MEDIAN_RE.search(log)
    if m:
        row["scan_wall_s"] = float(m.group(1)) / 1000.0
        row["scan_runs"] = len(m.group(3).split(","))
        row["scan_runs_ms"] = [float(x) / 1000.0 for x in m.group(3).split(",")]
    else:
        # Failed cells never log the median line; fall back to the raw
        # per-run walls so partial runs stay readable.
        walls = []
        try:
            for line in (outdir / "scan-walls.tsv").read_text().splitlines():
                parts = line.split()
                if len(parts) == 2:
                    walls.append(float(parts[1]) / 1000.0)
        except OSError:
            pass
        if walls:
            row["scan_wall_s"] = statistics.median(walls)
            row["scan_runs"] = len(walls)
            row["scan_runs_ms"] = walls
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
            r"probe round=\d+ rows=(\d+) state=\w+ (?:seq: seq: n=\d+ ok=\d+ mean=([\d.]+).*?conc(?:-P\d+)?: n=\d+ ok=\d+ mean=([\d.]+))",
            log,
        )
        if rounds:
            last = rounds[-1]
            row["probe_rows"] = int(last[0]) if last[0] else 0
            if last[1]:
                # Legacy curl-sweep format: full-list mean, no per-round
                # prefetch — prefetch stays absent (n/a), not zero.
                row["scan_thumb_mean"] = float(last[1])
    # Pool ALL per-round hyperfine lines the same way: median of round
    # medians (robust to single stalled runs and single stalled rounds).
    rh = _TRANSCRIPT_HYPERFINE_RE.findall(log)
    if rh:
        row["scan_pref_median"] = statistics.median(float(r[0]) for r in rh)
        row["scan_edit_median"] = statistics.median(float(r[1]) for r in rh)
        row["scan_pref_worst"] = max(float(r[0]) for r in rh)
        row["scan_edit_worst"] = max(float(r[1]) for r in rh)
        row["scan_rounds"] = len(rh)
    else:
        # Legacy no-hyperfine dialect: per-round single-shot timings.
        rh_single = re.findall(
            r"probe round=\d+ .*?hyperfine: prefetch single=([\d.]+) s \| edit-rating single=([\d.]+) s",
            log,
        )
        if rh_single:
            row["scan_pref_median"] = statistics.median(float(r[0]) for r in rh_single)
            row["scan_edit_median"] = statistics.median(float(r[1]) for r in rh_single)
            row["scan_pref_worst"] = max(float(r[0]) for r in rh_single)
            row["scan_edit_worst"] = max(float(r[1]) for r in rh_single)
            row["scan_rounds"] = len(rh_single)
    # Storm summaries (one per scan run for storm-* workloads).
    storms = _TRANSCRIPT_STORM_RE.findall(log)
    if storms:
        row["storms"] = []
        for s in storms:
            profile, kind = s[0], s[8] or None
            key = profile if not kind or profile.endswith(kind) else f"{profile}-{kind}"
            row["storms"].append(
                {
                    "key": key,
                    "done": int(s[1]),
                    "total": int(s[2]),
                    "median": float(s[3]),
                    "p95": float(s[4]),
                    "max": float(s[5]),
                    "failed": int(s[6]),
                    "wall": float(s[7]),
                }
            )
    verifies = _TRANSCRIPT_VERIFY_RE.findall(log)
    if verifies:
        row["storm_verify"] = [(int(m), int(t)) for m, t in verifies]


def _apply_post_scan(row: dict, log: str) -> None:
    """Post-scan (idle-server) hyperfine medians.

    v1 carries only during-scan events; these stay transcript fields.
    Idle server: mean ~= median, take median for consistency with the
    during-scan columns.
    """
    m = re.search(r"hyperfine compressed-get: mean=[\d.]+ s median=([\d.]+) s", log)
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


def _apply_stage_rows(row: dict, outdir: Path, log: str) -> None:
    """Observer drain tables: from server.log when the launcher kept it."""
    server_log = outdir / "server.log"
    drain_source = server_log if server_log.exists() else log
    _drains = parse_drain_tables(drain_source)
    row["drain_tables"] = _drains
    row["drains"] = _drains
    row["drain_stages"] = parse_drain_rows(drain_source)
    row["stages"] = row["drain_stages"]
    row["stage_rows"] = row["drain_stages"]
    row["observer_drain_tables"] = _drains


def parse_log(outdir: Path) -> dict:
    """Read one cell dir into a row, preferring its `events.jsonl` stream."""
    log = (outdir / "harness-a.log").read_text()
    row: dict = {"benchmark": outdir.name}
    _apply_setup(row, log)
    events = read_cell_events(outdir, log)
    if events is None:
        _apply_log_measurements(row, outdir, log)
    else:
        _apply_events(row, events, log)
    _apply_post_scan(row, log)
    _apply_stage_rows(row, outdir, log)
    return row


def parse_status(outdir: Path) -> dict:
    try:
        return json.loads((outdir / "status-final.json").read_text())
    except (OSError, json.JSONDecodeError) as exc:
        print(f"error: {outdir}: cannot read status-final.json: {exc}", file=sys.stderr)
        sys.exit(1)


def _storm_summary(storms: list, key: str) -> dict | None:
    """Aggregate per-run storm summaries: median of medians, worst of max."""
    runs = [s for s in storms if s["key"] == key]
    if not runs:
        return None
    return {
        "ops": f"{sum(s['done'] for s in runs)}/{sum(s['total'] for s in runs)}",
        "med": statistics.median(s["median"] for s in runs),
        "p95": max(s["p95"] for s in runs),
        "max": max(s["max"] for s in runs),
        "fail": sum(s["failed"] for s in runs),
    }


def _cell(row: dict, status: dict, storm_keys: list) -> dict:
    """Flatten one parsed cell into label-agnostic string/number fields.

    The row comes from :func:`parse_log`, so it already carries event-stream
    measurements when the cell has a complete `events.jsonl`.
    """
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
        "idle_thumb_ms": _ms(row.get("post_compressed")),
        "idle_fetch_ms": _ms(row.get("post_prefetch")),
        "idle_edit_ms": _ms(row.get("post_edit")),
    }
    for key in storm_keys:
        summary = _storm_summary(row.get("storms", []), key)
        if summary is None:
            cell[f"{key}_ops"] = "n/a"
            cell[f"{key}_med_ms"] = "n/a"
            cell[f"{key}_p95_ms"] = "n/a"
            cell[f"{key}_max_ms"] = "n/a"
            cell[f"{key}_fail"] = "n/a"
        else:
            cell[f"{key}_ops"] = summary["ops"]
            cell[f"{key}_med_ms"] = round(summary["med"] * 1000, 1)
            cell[f"{key}_p95_ms"] = round(summary["p95"] * 1000, 1)
            cell[f"{key}_max_ms"] = round(summary["max"] * 1000, 1)
            cell[f"{key}_fail"] = summary["fail"]
    if row.get("storm_verify"):
        cell["storm_verify"] = "; ".join(f"{m}/{t}" for m, t in row["storm_verify"])
    else:
        cell["storm_verify"] = "n/a"
    # Observer drain summary columns (per scan).
    drains = row.get("drain_tables", row.get("drains", []))

    def _stage_values(stage_name: str, field: str) -> str:
        values = []
        for table in drains:
            for stage in table.get("stages", []):
                if stage.get("stage") == stage_name:
                    values.append(stage.get(field))
        if not values:
            return "n/a"
        return ",".join(str(v) for v in values)

    for stage_name in ("tree_refresh_build", "filter_snapshot_clone"):
        for field in ("p50_us", "max_us"):
            key_us = f"{stage_name}_{field}"
            cell[key_us] = _stage_values(stage_name, field)
            cell[f"{stage_name}_{field}_us"] = cell[key_us]
            # millisecond mirror for latency-style columns
            vals = [v for v in (_stage_values(stage_name, field),) if v != "n/a"]
            if vals and vals[0] != "n/a":
                try:
                    ms_vals = ",".join(
                        str(round(int(x) / 1000.0, 1)) for x in cell[key_us].split(",")
                    )
                    cell[f"{stage_name}_{field}_ms"] = ms_vals
                except ValueError, AttributeError:
                    cell[f"{stage_name}_{field}_ms"] = "n/a"
            else:
                cell[f"{stage_name}_{field}_ms"] = "n/a"
            # alias without unit suffix
            cell[f"{stage_name}_{field.replace('_us', '')}"] = cell[key_us]
    return cell


def _storm_keys(storms: list) -> list:
    """Ordered distinct storm keys in one cell, base profiles first."""
    present = []
    for storm in storms:
        if storm["key"] not in present:
            present.append(storm["key"])
    order = ["storm-diff", "storm-read", "storm-thumb", "storm-idle"]
    base = [key for key in order if key in present]
    rest = sorted(key for key in present if key not in base)
    return base + rest


def _ms(value) -> object:
    return round(value * 1000, 1) if value is not None else "n/a"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("outdirs", nargs="+", help="bench OUT dirs")
    args = parser.parse_args()

    parsed = []
    for raw in args.outdirs:
        outdir = Path(raw)
        if not (outdir / "harness-a.log").exists():
            print(f"error: {outdir}: no harness-a.log", file=sys.stderr)
            sys.exit(1)
        row = parse_log(outdir)
        # Emit per-scan stage rows as machine-readable JSON in the outdir.
        try:
            (outdir / "stages.json").write_text(
                json.dumps(row.get("drain_stages", []), indent=2) + "\n"
            )
        except OSError as exc:
            print(f"warn: {outdir}: cannot write stages.json: {exc}", file=sys.stderr)
        parsed.append((row, parse_status(outdir)))

    storm_union: list = []
    for row, _ in parsed:
        for key in _storm_keys(row.get("storms", [])):
            if key not in storm_union:
                storm_union.append(key)
    cells = [_cell(row, status, storm_union) for row, status in parsed]

    profiles = storm_union
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
        "idle_thumb_ms",
        "idle_fetch_ms",
        "idle_edit_ms",
        "tree_refresh_build_p50_us",
        "tree_refresh_build_max_us",
        "filter_snapshot_clone_p50_us",
        "filter_snapshot_clone_max_us",
        "storm_verify",
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
    print("median/worst latency: pre[fetch], [thumb]nail, metadata[edit], scan and idle")


if __name__ == "__main__":
    main()
