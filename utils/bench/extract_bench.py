#!/usr/bin/env python3
"""Extract benchmark summary from one or more bench output dirs.

Usage:
    utils/bench/extract_bench.py /tmp/.../bench-after-N4-100 [...]

Reads events.jsonl, harness-a.log, status-final.json and hyperfine.json
from each OUT dir (as produced by the bench workload) and prints one
metric per line, one column per cell: image count/bytes, scan wall time +
sec/MB, server stage costs, and the full request-latency matrix — every
background (idle, scan, uscan) x access condition (prefetch, thumb, edit,
storm_read, storm_thumb, storm_edit) x statistic (p50, p95, max), plus the
failed-op counts of every storm kind. See the BACKGROUNDS/ACCESSES/STATS
block for the column scheme.

Measurements prefer the cell's structured `events.jsonl` stream (see
:func:`read_events`); without a usable stream the numbers come from the
per-round artifacts (hyperfine exports, probe/storm TSVs, scan walls),
with the transcript's slim status lines saying which rounds ran. Both
paths report identical values for the same run.

Scan counters come from status-final.json and are validity-checked only:
a non-completed state, any failure, or a processed/matched mismatch prints
a WARN line to stderr instead of a table row. Server stage costs come from
the `observe drain:` tables in server.log (see :func:`parse_drain_tables`).
Exit non-zero if a dir is missing expected artifacts.
"""

import argparse
import dataclasses
import json
import math
import re
import statistics
import sys
from pathlib import Path

# --- report matrix -----------------------------------------------------------
# One latency column per background x access condition x statistic. A row
# carries every column; a condition the cell does not measure is `n/a`.
#
#   bg      idle    post-scan baselines (server at rest)
#           scan    during-scan probes and bursts on control/storm cells
#           uscan   during-scan probes on the upload profile (the upload
#                   stimulus runs its own populate scan)
#   access  prefetch, thumb, edit          request-path probes
#           storm_read, storm_thumb, storm_edit   concurrent bursts
#   stat    p50, p95, max
BACKGROUNDS = ("idle", "scan", "uscan")
ACCESSES = ("prefetch", "thumb", "edit", "storm_read", "storm_thumb", "storm_edit")
STATS = ("p50", "p95", "max")
STORM_KINDS = ("read", "thumb", "edit")
# Only these backgrounds run storm bursts; upload never does.
STORM_BACKGROUNDS = ("idle", "scan")

LATENCY_COLUMNS = tuple(
    f"{bg}_{access}_{stat}_ms" for bg in BACKGROUNDS for access in ACCESSES for stat in STATS
)
STORM_FAILED_COLUMNS = tuple(
    f"{bg}_storm_{kind}_failed" for bg in STORM_BACKGROUNDS for kind in STORM_KINDS
)

# Report access name -> storm op kind the workload records (`diff` is the
# edit-rating storm; its report columns are named `storm_edit`).
STORM_OP_KIND = {"read": "read", "thumb": "thumb", "edit": "diff"}

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
# short. Counts come from the slim status lines; the numbers live
# in the artifacts.
_TRANSCRIPT_SCAN_RE = re.compile(r"scan run \d+/\d+: state=\w+ polls=\d+ wall_ms=\d+")
_TRANSCRIPT_SCAN_MEDIAN_RE = re.compile(
    r"scan wall median_ms=([\d.]+) over (\d+) run\(s\): ([\d,]+)"
)
_TRANSCRIPT_VERIFY_RE = re.compile(r"^.*storm verify: matched (\d+)/(\d+) canary writes", re.M)

# The transcript logs what ran (one status line per probe round, one
# summary line per storm); the numbers live in the per-round artifacts.
# The readers below parse those artifacts with the same math the workload
# and event paths use, so every path reports identical values.
_PROBE_ROUND_RE = re.compile(
    r"^(?:\d\d:\d\d:\d\d )?probe round=(\d+) rows=(\d+) state=(\w+)\s*$", re.M
)
_PROBE_ROUND_FAILED_RE = re.compile(
    r"^(?:\d\d:\d\d:\d\d )?probe round=(\d+) rows=(\d+) state=(\w+) hyperfine FAILED", re.M
)
_STORM_SUMMARY_RE = re.compile(
    r"^.*storm: profile=(\S+) ops=(\d+)/(\d+) failed=(\d+) wall=([\d.]+)s(?: kind=(\S+))?",
    re.M,
)
_ROUND_FILE_RE = re.compile(r"rounds-run-(\d+)/\w+-round-(\d+)\.")


def _ordered_round_files(outdir: Path, stem: str, suffix: str) -> list:
    """Per-round artifact paths across runs, ordered by (run, round)."""
    found = []
    for path in outdir.glob(f"rounds-run-*/{stem}-*{suffix}"):
        match = _ROUND_FILE_RE.search(str(path.relative_to(outdir)))
        if match:
            found.append((int(match.group(1)), int(match.group(2)), path))
    return [path for _, _, path in sorted(found)]


def _round_hyperfine_meds(outdir: Path) -> list:
    """(prefetch_median, edit_median) per probe round, oldest run first.

    Reads the same exports the workload and the event stream read, so
    every path reports the identical float.
    """
    rounds = []
    for path in _ordered_round_files(outdir, "hyperfine-round", ".json"):
        try:
            document = json.loads(path.read_text())
        except OSError, ValueError:
            continue
        results = document.get("results") if isinstance(document, dict) else None
        if not isinstance(results, list):
            continue
        by_name = {result.get("name"): result for result in results if isinstance(result, dict)}
        pre, edit = by_name.get("prefetch"), by_name.get("edit-rating")
        if pre is None or edit is None:
            continue
        pre_med = _reported_median(pre)
        edit_med = _reported_median(edit)
        if pre_med is None or edit_med is None:
            continue
        rounds.append((pre_med, edit_med))
    return rounds


def _round_fetch_means(outdir: Path) -> list:
    """(samples, mean) per probe round, oldest run first.

    The mean covers every sampled op including failures, exactly like the
    workload's own average over the same file.
    """
    rounds = []
    for path in _ordered_round_files(outdir, "probe-round", ".tsv"):
        try:
            text = path.read_text()
        except OSError:
            continue
        times = []
        for line in text.splitlines():
            parts = line.split("\t")
            if len(parts) < 3:
                continue
            try:
                times.append(float(parts[2]))
            except ValueError:
                continue
        if times:
            rounds.append((len(times), sum(times) / len(times)))
    return rounds


def _round_storm_aggs(outdir: Path) -> list:
    """(median, p95, max, failed) per storm round, oldest run first.

    Same multiset, same order, same functions as the workload summary,
    so the aggregates match the event stream exactly.
    """
    aggs = []
    for path in _ordered_round_files(outdir, "storm-round", ".tsv"):
        try:
            text = path.read_text()
        except OSError:
            continue
        latencies, failed = [], 0
        for line in text.splitlines():
            parts = line.split("\t")
            if len(parts) < 4:
                continue
            try:
                latencies.append(float(parts[2]))
            except ValueError:
                continue
            if parts[3] != "200":
                failed += 1
        if not latencies:
            continue
        ordered = sorted(latencies)
        aggs.append(
            (
                statistics.median(ordered),
                percentile(ordered, 95),
                ordered[-1],
                failed,
            )
        )
    return aggs


def _reported_median(result: dict) -> float | None:
    """A hyperfine export's own median for one benchmark result.

    Mirrors the workload's wall-clock reading: the flat `median` field,
    else the nested `summary.time_wall_clock.median`.
    """
    if not isinstance(result, dict):
        return None
    median = result.get("median")
    if isinstance(median, (int, float)):
        return float(median)
    nested = result.get("summary", {})
    if isinstance(nested, dict):
        nested = nested.get("time_wall_clock", {})
    if isinstance(nested, dict):
        median = nested.get("median")
        if isinstance(median, (int, float)):
            return float(median)
    return None


def _hyperfine_reported_medians(path: Path) -> dict:
    """{bench name: hyperfine-reported median} from a post-scan export."""
    try:
        document = json.loads(Path(path).read_text())
    except OSError, ValueError:
        return {}
    results = document.get("results") if isinstance(document, dict) else None
    if not isinstance(results, list):
        return {}
    medians = {}
    for result in results:
        if not isinstance(result, dict):
            continue
        name = result.get("name")
        median = _reported_median(result)
        if isinstance(name, str) and median is not None:
            medians[name] = median
    return medians


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


def _probe_rounds(log: str, failed: bool = True) -> list:
    """(round, rows) per probe status line, in log order.

    A hyperfine failure still fetched, so failed rounds count for fetch
    tails but not for hyperfine medians — mirroring what the event stream
    records for the same cell.
    """
    rounds = [(int(n), int(r)) for n, r, _ in _PROBE_ROUND_RE.findall(log)]
    if failed:
        rounds += [(int(n), int(r)) for n, r, _ in _PROBE_ROUND_FAILED_RE.findall(log)]
    return rounds


def events_complete(events: Events, log: str) -> tuple:
    """Check the stream against the transcript; ``(ok, reason)``."""
    checks = (
        ("probe rounds", len(fetch_rounds(events)), len(_probe_rounds(log))),
        (
            "hyperfine rounds",
            len(hyperfine_rounds(events)),
            len(_probe_rounds(log, failed=False)),
        ),
        ("scan runs", len(events.scan_states), len(_TRANSCRIPT_SCAN_RE.findall(log))),
        (
            "storm summaries",
            len(events.storm_summaries),
            len(_STORM_SUMMARY_RE.findall(log)),
        ),
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
        print(f"warn: {path}: {exc}; falling back to per-round artifacts", file=sys.stderr)
        return None
    except EventsError as exc:
        print(f"warn: {exc}; falling back to per-round artifacts", file=sys.stderr)
        return None
    complete, reason = events_complete(events, log)
    if not complete:
        print(
            f"warn: {path}: incomplete ({reason}); falling back to per-round artifacts",
            file=sys.stderr,
        )
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


def percentile(values: list, pct: float) -> float:
    """Linear-interpolated percentile of already-collected samples.

    Same interpolation the workload uses for its storm p95, so a tail
    computed here over raw samples matches the tail the workload prints for
    the same values.
    """
    ordered = sorted(values)
    if not ordered:
        raise ValueError("percentile of an empty sample")
    if len(ordered) == 1:
        return ordered[0]
    rank = (len(ordered) - 1) * pct / 100.0
    low = math.floor(rank)
    high = math.ceil(rank)
    if low == high:
        return ordered[low]
    return ordered[low] + (ordered[high] - ordered[low]) * (rank - low)


def hyperfine_runs(path) -> dict:
    """Per-run wall-clock seconds per benchmark in a hyperfine export.

    Returns ``{benchmark name: [seconds, ...]}``; an absent or unreadable
    export yields ``{}``. Reads the flat `times` list hyperfine < 2 writes
    and the `measurements[].time_wall_clock.value` list hyperfine 2 writes.
    """
    try:
        document = json.loads(Path(path).read_text())
    except OSError, json.JSONDecodeError:
        return {}
    results = document.get("results") if isinstance(document, dict) else None
    if not isinstance(results, list):
        return {}
    runs: dict = {}
    for result in results:
        if not isinstance(result, dict):
            continue
        name = result.get("name")
        if not isinstance(name, str):
            continue
        values = []
        times = result.get("times")
        if isinstance(times, list):
            values = [float(value) for value in times if isinstance(value, (int, float))]
        else:
            for measurement in result.get("measurements") or []:
                wall = (measurement.get("time_wall_clock") or {}) if measurement else {}
                value = wall.get("value")
                if isinstance(value, (int, float)):
                    values.append(float(value))
        runs[name] = values
    return runs


def storm_kind(storm: dict) -> str:
    """Storm op kind; a storm-idle run defaults to the read kind."""
    kind = storm.get("kind")
    if kind:
        return str(kind)
    profile = str(storm.get("profile", ""))
    if profile == "storm-idle":
        return "read"
    return profile.removeprefix("storm-")


def storm_background(storm: dict) -> str:
    """`idle` for a tree-at-rest burst, `scan` for a concurrent one."""
    return "idle" if storm.get("profile") == "storm-idle" else "scan"


def _storm_from_event(event: dict) -> dict:
    profile = event["profile"]
    kind = event.get("kind") or None
    key = profile if not kind or profile.endswith(kind) else f"{profile}-{kind}"
    return {
        "key": key,
        "profile": profile,
        "kind": kind,
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
        row["probe_round_thumb"] = thumb
    rounds = hyperfine_rounds(events)
    if rounds:
        prefetch = [event["prefetch_median"] for event in rounds]
        edit = [event["edit_median"] for event in rounds]
        row["scan_pref_median"] = statistics.median(prefetch)
        row["scan_edit_median"] = statistics.median(edit)
        row["scan_pref_worst"] = max(prefetch)
        row["scan_edit_worst"] = max(edit)
        row["scan_rounds"] = len(rounds)
        row["probe_round_prefetch"] = prefetch
        row["probe_round_edit"] = edit
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


def _apply_artifact_measurements(row: dict, outdir: Path, log: str) -> None:
    """Measurement fields from the per-round artifacts.

    Runs when the event stream is absent or unusable. The transcript
    only says which rounds ran (slim status lines); every number comes
    from the artifacts the workload wrote. Identity, scan walls, upload
    p50s and verify counts still come from transcript lines, which carry
    no floating-point stats.
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
    # Per-round fetch means come from the probe TSVs; the transcript only
    # says which rounds ran. Pairing is positional: every run writes one
    # TSV per round it executes and logs one line per round, in order.
    slim = _probe_rounds(log)
    means = _round_fetch_means(outdir)
    if slim and means:
        pairs = list(zip(slim, means))
        total_n = sum(n for _, (n, _) in pairs)
        thumb = [_recorded(mean, 4) for _, (_, mean) in pairs]
        row["probe_rows"] = max(rows for _, rows in slim)
        row["probe_rounds"] = len(slim)
        row["scan_thumb_mean"] = (
            sum(n * m for (_, (n, _)), m in zip(pairs, thumb)) / total_n if total_n else 0.0
        )
        row["scan_thumb_n"] = total_n
        row["scan_thumb_worst"] = max(thumb)
        row["probe_round_thumb"] = thumb
    elif slim:
        row["probe_rows"] = max(rows for _, rows in slim)
        row["probe_rounds"] = len(slim)
    # Per-round hyperfine medians come from the per-round export JSONs —
    # the same files the workload wrote and the event stream read.
    meds = _round_hyperfine_meds(outdir)
    if meds:
        prefetch = [pre for pre, _ in meds]
        edit = [ed for _, ed in meds]
        row["scan_pref_median"] = statistics.median(prefetch)
        row["scan_edit_median"] = statistics.median(edit)
        row["scan_pref_worst"] = max(prefetch)
        row["scan_edit_worst"] = max(edit)
        row["scan_rounds"] = len(meds)
        row["probe_round_prefetch"] = prefetch
        row["probe_round_edit"] = edit
    # Storm summaries (one per scan run for storm-* workloads): the slim
    # log line carries identity (profile, kind, done, total, failed,
    # wall); the medians come from aggregating the per-round storm TSVs
    # with the workload's own functions.
    slim = _STORM_SUMMARY_RE.findall(log)
    aggs = _round_storm_aggs(outdir)
    if slim and aggs:
        row["storms"] = []
        for s, (median, p95, mx, failed) in zip(slim, aggs):
            event = {
                "profile": s[0],
                "kind": s[5] or None,
                "done": int(s[1]),
                "total": int(s[2]),
                "median": median,
                "p95": p95,
                "max": mx,
                "failed": int(s[3]),
                "wall": float(s[4]),
            }
            row["storms"].append(_storm_from_event(event))
    verifies = _TRANSCRIPT_VERIFY_RE.findall(log)
    if verifies:
        row["storm_verify"] = [(int(m), int(t)) for m, t in verifies]


def _apply_post_scan(row: dict, log: str, outdir: Path | None = None) -> None:
    """Post-scan (idle-server) hyperfine medians, read from the export.

    The export carries the exact medians the transcript used to log;
    reading it keeps one source of truth for every cell. Cells without
    a post-scan export leave these absent.
    """
    if outdir is None:
        return
    medians = _hyperfine_reported_medians(outdir / "hyperfine.json")
    for access, bench in _POST_SCAN_BENCH.items():
        if bench in medians:
            row[_POST_SCAN_FIELDS[access]] = medians[bench]


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


_POST_SCAN_BENCH = {"prefetch": "prefetch", "thumb": "compressed-get", "edit": "edit-rating"}
_POST_SCAN_FIELDS = {"prefetch": "post_prefetch", "thumb": "post_compressed", "edit": "post_edit"}
_PROBE_P50_FIELDS = {
    "prefetch": ("scan_pref_median", "scan_pref_mean"),
    "thumb": ("scan_thumb_mean",),
    "edit": ("scan_edit_median", "scan_edit_mean"),
}
_PROBE_ROUND_FIELDS = {
    "prefetch": "probe_round_prefetch",
    "thumb": "probe_round_thumb",
    "edit": "probe_round_edit",
}


def _apply_matrix_samples(row: dict, outdir: Path, profile) -> None:
    """Collect the per-round and per-run values behind the matrix tails.

    These feed the `p95`/`max` cells only; the `p50` columns keep the
    pooling the report has always used, so the recorded values stay
    comparable with the checked-in baselines.

    - ``idle_*``: every timed run of the post-scan ``hyperfine.json``
      export — the same export whose medians the transcript logs. It
      carries the `compressed-get` (thumb), `prefetch` and `edit-rating`
      (edit) repetitions, so p95/max describe every run of the cell and
      the report's ``:median`` row pools those runs across repeats. A cell
      that predates the export falls back to its recorded post-scan median
      as a single sample.
    - ``scan_*`` / ``uscan_*``: the per-round probe summaries — one
      prefetch hyperfine median, one edit-rating hyperfine median and one
      thumbnail fetch mean per probe round. Those are the values the cell
      records for each round, so the tails are percentiles across rounds.
      The bucket comes from the cell profile, never from parsing: ``upload``
      rows carry ``uscan_*`` and every other profile ``scan_*``.
    """
    samples: dict = {}
    post = hyperfine_runs(outdir / "hyperfine.json")
    for access, bench in _POST_SCAN_BENCH.items():
        values = post.get(bench, [])
        if not values:
            median = row.get(_POST_SCAN_FIELDS[access])
            values = [median] if median is not None else []
        if values:
            samples[f"idle_{access}"] = [value * 1000.0 for value in values]

    bucket = "uscan" if profile == "upload" else "scan"
    row["probe_bucket"] = bucket
    for access, field in _PROBE_ROUND_FIELDS.items():
        values = row.get(field) or []
        if values:
            samples[f"{bucket}_{access}"] = [value * 1000.0 for value in values]
    row["samples"] = samples


def parse_log(outdir: Path, profile=None) -> dict:
    """Read one cell dir into a row, preferring its `events.jsonl` stream.

    ``profile`` is the cell's WORKLOAD profile. It decides whether the
    during-scan probes belong to the `scan` or the `uscan` background; it is
    the battery's label for the cell, not something the artifacts are
    searched for.
    """
    log = (outdir / "harness-a.log").read_text()
    row: dict = {"benchmark": outdir.name}
    _apply_setup(row, log)
    events = read_cell_events(outdir, log)
    if events is None:
        _apply_artifact_measurements(row, outdir, log)
    else:
        _apply_events(row, events, log)
    _apply_post_scan(row, log, outdir)
    _apply_matrix_samples(row, outdir, profile)
    _apply_stage_rows(row, outdir, log)
    return row


def parse_status(outdir: Path) -> dict:
    try:
        return json.loads((outdir / "status-final.json").read_text())
    except (OSError, json.JSONDecodeError) as exc:
        print(f"error: {outdir}: cannot read status-final.json: {exc}", file=sys.stderr)
        sys.exit(1)


def _storm_summary(storms: list, background: str, kind: str) -> dict | None:
    """Aggregate a background's storm runs for one op kind.

    A storm cell records one summary per run, so the aggregate keeps the
    established convention: median of run medians, worst of run p95s, worst
    of run maxes, failures summed over runs.
    """
    runs = [
        storm
        for storm in storms
        if storm_background(storm) == background and storm_kind(storm) == kind
    ]
    if not runs:
        return None
    return {
        "ops": f"{sum(s['done'] for s in runs)}/{sum(s['total'] for s in runs)}",
        "med": statistics.median(s["median"] for s in runs),
        "p95": max(s["p95"] for s in runs),
        "max": max(s["max"] for s in runs),
        "fail": sum(s["failed"] for s in runs),
    }


_STAT_FIELDS = {"p50": "med", "p95": "p95", "max": "max"}


def _matrix_latency(row: dict, background: str, access: str, stat: str):
    """One matrix latency cell: seconds from a summary, or pooled samples."""
    if access.startswith("storm_"):
        kind = STORM_OP_KIND[access.removeprefix("storm_")]
        summary = _storm_summary(row.get("storms", []), background, kind)
        if summary is None:
            return "n/a"
        return _ms(summary[_STAT_FIELDS[stat]])
    if background in ("scan", "uscan") and row.get("probe_bucket") != background:
        # During-scan probes belong to exactly one background: the cell's
        # profile decides, so an upload row leaves `scan_*` empty and vice
        # versa.
        return "n/a"
    if stat == "p50":
        fields = (_POST_SCAN_FIELDS[access],) if background == "idle" else _PROBE_P50_FIELDS[access]
        value = next(
            (row[field] for field in fields if row.get(field) is not None),
            None,
        )
        return _ms(value)
    samples = (row.get("samples") or {}).get(f"{background}_{access}")
    if not samples:
        return "n/a"
    if stat == "max":
        return round(max(samples), 1)
    return round(percentile(samples, 95), 1)


def _matrix_failed(row: dict, background: str, kind: str):
    """Failures a storm kind recorded in one background; n/a when it did not run."""
    summary = _storm_summary(row.get("storms", []), background, STORM_OP_KIND[kind])
    return "n/a" if summary is None else summary["fail"]


def _cell(row: dict, status: dict) -> dict:
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
    }
    for background in BACKGROUNDS:
        for access in ACCESSES:
            for stat in STATS:
                cell[f"{background}_{access}_{stat}_ms"] = _matrix_latency(
                    row, background, access, stat
                )
    for background in STORM_BACKGROUNDS:
        for kind in STORM_KINDS:
            cell[f"{background}_storm_{kind}_failed"] = _matrix_failed(row, background, kind)
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


def _ms(value) -> object:
    return round(value * 1000, 1) if value is not None else "n/a"


def main(argv=None) -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("outdirs", nargs="+", help="bench OUT dirs")
    parser.add_argument(
        "--profile",
        help="WORKLOAD profile of the cell(s); decides scan vs uscan probe bucket (default: scan)",
    )
    args = parser.parse_args(argv)

    parsed = []
    for raw in args.outdirs:
        outdir = Path(raw)
        if not (outdir / "harness-a.log").exists():
            print(f"error: {outdir}: no harness-a.log", file=sys.stderr)
            sys.exit(1)
        row = parse_log(outdir, profile=args.profile)
        # Emit per-scan stage rows as machine-readable JSON in the outdir.
        try:
            (outdir / "stages.json").write_text(
                json.dumps(row.get("drain_stages", []), indent=2) + "\n"
            )
        except OSError as exc:
            print(f"warn: {outdir}: cannot write stages.json: {exc}", file=sys.stderr)
        parsed.append((row, parse_status(outdir)))

    cells = [_cell(row, status) for row, status in parsed]

    for i, cell in enumerate(cells, 1):
        print(f"[{i}] name: {cell['name']}")
        print(f"    {cell['setup']}")
    labels = [
        "scan_time_s",
        "s/MB",
        "MB",
        "scan_runs_s",
        *LATENCY_COLUMNS,
        *STORM_FAILED_COLUMNS,
        "tree_refresh_build_p50_us",
        "tree_refresh_build_max_us",
        "filter_snapshot_clone_p50_us",
        "filter_snapshot_clone_max_us",
        "storm_verify",
    ]
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
    print(
        "matrix: {idle,scan,uscan} x {prefetch,thumb,edit,storm_read,storm_thumb,storm_edit}"
        " x {p50,p95,max}; failed counts per storm kind"
    )


if __name__ == "__main__":
    main()
