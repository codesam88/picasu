#!/usr/bin/env python3
"""Extract a per-rep benchmark record from one or more bench output dirs.

Usage:
    utils/bench/extract_bench.py /tmp/.../bench-after-N4-100 [...]

Reads events.jsonl, harness-a.log, status-final.json and hyperfine.json
from each OUT dir (as produced by the bench workload) and prints, for each
cell, its identity, validity and one row per metric (p50, p95, max or sum
with the raw sample count). Measurements prefer the cell's structured
`events.jsonl` stream (see :func:`read_events`); without a usable stream
the numbers come from the per-round artifacts (hyperfine exports, probe/storm
TSVs, scan walls), with the transcript's slim status lines saying which
rounds ran. Both paths report identical values for the same run.

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

import config
import metrics

# --- measurement axes --------------------------------------------------------
# Raw samples are tagged with the axis values the contract defines:
#   background  idle (server at rest), scan (a scan is running)
#   access      prefetch, thumb, edit, scroll_bar, scroll_rows, scroll_data
#   stat        p50, p95, max (sum for counts), derived by the reducer
BACKGROUNDS = metrics.BACKGROUNDS
ACCESSES = metrics.ACCESSES
STATS = metrics.STATS

# Process-level cost recorded by the cell orchestrator while the server is
# alive: peak resident set and the snapshot store's apparent size.
METRICS_COLUMNS = (
    "vm_hwm_mb",
    "temp_db_mb",
    "index_v5_mb",
    "cache_db_mb",
    "compressed_mb",
)


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


_COUNTERS_LINE = re.compile(r"observe counters:\s*(.*)")


def parse_counters(log) -> list:
    """Parse ``observe counters:`` lines into one dict per drain table.

    Each dict maps a counter name to its exact count for that scan. A cell
    without the line (observer stats off) yields an empty list.
    """
    if isinstance(log, (str, bytes)):
        text = log.decode() if isinstance(log, bytes) else log
    else:
        try:
            text = Path(log).read_text()
        except OSError, TypeError:
            text = str(log)
    counts: list = []
    for line in text.splitlines():
        match = _COUNTERS_LINE.search(line)
        if match is None:
            continue
        values = {}
        for part in match.group(1).split():
            name, _, raw = part.partition("=")
            if not raw:
                continue
            try:
                values[name] = int(raw)
            except ValueError:
                continue
        counts.append(values)
    return counts


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
    "scroll_probe": (
        "scroll_probes",
        (),
        (),
    ),
    "phase_guard": (
        "phase_guards",
        ("phase", "valid", "wall_ms"),
        ("wall_ms", "scanned_start", "scanned_end", "processed_start", "processed_end"),
    ),
    "warmup": (
        "warmups",
        ("valid", "wall_ms"),
        ("wall_ms",),
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
    scroll_probes: list = dataclasses.field(default_factory=list)
    phase_guards: list = dataclasses.field(default_factory=list)
    warmups: list = dataclasses.field(default_factory=list)


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
    r"^.*storm: profile=(\S+) ops=(\d+)/(\d+)(?: \([^)]*\))? "
    r"failed=(\d+) wall=([\d.]+)s(?: kind=(\S+))?",
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


def _round_scroll_samples(outdir: Path) -> dict:
    """Raw scroll-probe samples (ms) across per-round artifacts, split by bg.

    Per-round artifacts (rounds-run-*/scroll-round-*.tsv) run during scan;
    the post-settle file (scroll-probe.tsv) runs at idle.
    """
    samples = {
        "scan": {"scrollbar": [], "rows": [], "data": []},
        "idle": {"scrollbar": [], "rows": [], "data": []},
    }
    for path in outdir.glob("rounds-run-*/scroll-round-*.tsv"):
        _read_scroll_tsv(path, samples["scan"])
    candidate = outdir / "scroll-probe.tsv"
    if candidate.is_file():
        _read_scroll_tsv(candidate, samples["idle"])
    return samples


def _read_scroll_tsv(path: Path, bucket: dict) -> None:
    try:
        text = path.read_text()
    except OSError:
        return
    for line in text.splitlines():
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        kind = parts[0].strip().lower().replace("-", "_")
        key = None
        if kind in ("scrollbar", "scroll_bar"):
            key = "scrollbar"
        elif kind in ("rows", "scroll_rows"):
            key = "rows"
        elif kind in ("data", "scroll_data"):
            key = "data"
        if key is None:
            continue
        try:
            bucket[key].append(float(parts[2]) * 1000.0)
        except ValueError:
            continue


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


_STORM_ACCESS_ALIASES = {"read": "prefetch", "diff": "edit"}


def storm_kind(storm: dict) -> str:
    """Metric access driven by a storm run: prefetch, thumb or edit.

    Profiles name the read access as `read`; events may carry the request
    kind (`prefetch`). Both normalize to the metric access so samples and
    summaries of one run share a group.
    """
    kind = storm.get("kind")
    if not kind:
        kind = config.storm_profile_access(str(storm.get("profile", ""))) or "prefetch"
    kind = str(kind)
    return _STORM_ACCESS_ALIASES.get(kind, kind)


def storm_background(storm: dict) -> str:
    """`idle` for a tree-at-rest burst, `scan` for a concurrent one."""
    return "idle" if config.is_idle_storm(str(storm.get("profile", ""))) else "scan"


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
    phase_guards = getattr(events, "phase_guards", []) or []
    probe_guards = [e for e in phase_guards if e.get("phase") == "probe"]
    row["probe_rounds_total"] = len(probe_guards)
    row["probe_rounds_valid"] = sum(1 for e in probe_guards if e.get("valid") is True)
    scan_wall = row.get("scan_wall_s")
    scan_runs = row.get("scan_runs")
    if scan_wall and scan_runs and probe_guards:
        total_wall_ms = float(scan_wall) * 1000.0 * int(scan_runs)
        valid_wall_ms = sum(
            int(e.get("wall_ms") or 0) for e in probe_guards if e.get("valid") is True
        )
        pct = 100.0 * valid_wall_ms / total_wall_ms if total_wall_ms else 0.0
        if pct > 100.0:
            pct = 100.0
        row["probe_coverage_pct"] = round(pct, 1)
    else:
        row["probe_coverage_pct"] = "n/a"
    warmups = getattr(events, "warmups", []) or []
    if warmups:
        row["warmup_valid"] = "true" if all(e.get("valid") is True for e in warmups) else "false"
    else:
        row["warmup_valid"] = "n/a"
    invalid_guards = [e for e in phase_guards if e.get("valid") is False]
    invalid_warmups = [e for e in warmups if e.get("valid") is False]
    row["phase_invalid"] = len(invalid_guards) + len(invalid_warmups)
    reasons = []
    for event in invalid_guards + invalid_warmups:
        reason = event.get("reason")
        if reason and reason != "ok" and reason not in reasons:
            reasons.append(reason)
    row["phase_invalid_reasons"] = "; ".join(reasons) if reasons else "n/a"
    row.setdefault("storm_coverage_pct", "n/a")
    if events.storm_summaries:
        row["storms"] = [_storm_from_event(event) for event in events.storm_summaries]
        coverages = [event.get("coverage_pct") for event in events.storm_summaries]
        numeric_coverages = []
        for value in coverages:
            if isinstance(value, bool) or not isinstance(value, (int, float)):
                continue
            numeric_coverages.append(float(value))
        if numeric_coverages:
            row["storm_coverage_pct"] = statistics.median(numeric_coverages)
    verifies = storm_verifies(events)
    if verifies:
        row["storm_verify"] = verifies
    # Scroll probe samples (raw per-call latencies in ms), split by background.
    bar_samples = []
    rows_samples = []
    data_samples = []
    by_bg = {
        "idle": {"scrollbar": [], "rows": [], "data": []},
        "scan": {"scrollbar": [], "rows": [], "data": []},
    }
    for event in getattr(events, "scroll_probes", []) or []:
        bg = event.get("background")
        if bg not in by_bg:
            phase = event.get("phase")
            bg = "scan" if phase == "during" else "idle"
        if bg not in by_bg:
            bg = "scan"
        bar = event.get("scrollbar_ms") or event.get("scroll_bar_ms") or []
        rows = event.get("rows_ms") or event.get("scroll_rows_ms") or []
        data = event.get("data_ms") or event.get("scroll_data_ms") or []
        bar_samples.extend(bar)
        rows_samples.extend(rows)
        data_samples.extend(data)
        by_bg[bg]["scrollbar"].extend(bar)
        by_bg[bg]["rows"].extend(rows)
        by_bg[bg]["data"].extend(data)
    row["scroll_bar_samples"] = bar_samples
    row["scroll_rows_samples"] = rows_samples
    row["scroll_data_samples"] = data_samples
    row["scroll_samples_by_bg"] = by_bg


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
    scroll_samples = _round_scroll_samples(outdir)
    row["scroll_bar_samples"] = (
        scroll_samples["scan"]["scrollbar"] + scroll_samples["idle"]["scrollbar"]
    )
    row["scroll_rows_samples"] = scroll_samples["scan"]["rows"] + scroll_samples["idle"]["rows"]
    row["scroll_data_samples"] = scroll_samples["scan"]["data"] + scroll_samples["idle"]["data"]
    row["scroll_samples_by_bg"] = scroll_samples


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
    row["observer_counters"] = parse_counters(drain_source)


def _apply_cell_metrics(row: dict, outdir: Path) -> None:
    """Process-level cell metrics written by the orchestrator.

    `cell-metrics.json` is absent for cells recorded before the metrics
    existed; both report columns then stay `n/a`.
    """
    try:
        document = json.loads((outdir / "cell-metrics.json").read_text())
    except OSError, json.JSONDecodeError, ValueError:
        document = {}
    if not isinstance(document, dict):
        document = {}
    for column in METRICS_COLUMNS:
        row[column] = document.get(column)


_POST_SCAN_BENCH = {"prefetch": "prefetch", "thumb": "compressed-get", "edit": "edit-rating"}
_POST_SCAN_FIELDS = {"prefetch": "post_prefetch", "thumb": "post_compressed", "edit": "post_edit"}
_PROBE_ROUND_FIELDS = {
    "prefetch": "probe_round_prefetch",
    "thumb": "probe_round_thumb",
    "edit": "probe_round_edit",
}


_UPLOAD_FILE_RE = re.compile(r"upload file=\S+ submit_ms=([\d.]+) visible_ms=([\d.]+)")


def _round_hyperfine_runs(outdir: Path) -> tuple[dict, dict]:
    """All timed runs per probe access and the round count behind each."""
    values = {"prefetch": [], "edit": []}
    counts = {"prefetch": 0, "edit": 0}
    for path in _ordered_round_files(outdir, "hyperfine-round", ".json"):
        runs = hyperfine_runs(path)
        try:
            document = json.loads(path.read_text())
        except OSError, ValueError:
            document = {}
        results = document.get("results") if isinstance(document, dict) else []
        for access, bench in (("prefetch", "prefetch"), ("edit", "edit-rating")):
            found = [float(value) for value in runs.get(bench, [])]
            if not found:
                result = next(
                    (
                        item
                        for item in results
                        if isinstance(item, dict) and item.get("name") == bench
                    ),
                    None,
                )
                median = _reported_median(result) if result else None
                found = [median] if median is not None else []
            if found:
                values[access].extend(found)
                counts[access] += 1
    return values, counts


def _round_fetch_samples(outdir: Path) -> tuple[list, int]:
    """Every raw thumbnail fetch (seconds) and the number of rounds."""
    values = []
    count = 0
    for path in _ordered_round_files(outdir, "probe-round", ".tsv"):
        found = []
        try:
            text = path.read_text()
        except OSError:
            continue
        for line in text.splitlines():
            parts = line.split("\t")
            if len(parts) < 3:
                continue
            try:
                found.append(float(parts[2]))
            except ValueError:
                continue
        if found:
            values.extend(found)
            count += 1
    return values, count


def _storm_raw_from_artifacts(outdir: Path, profile, row=None) -> dict:
    """Raw op latencies, grouped by the background/kind of each storm run.

    The per-run summaries are paired positionally with the storm TSVs, so a
    caller without an explicit `profile` still gets the recorded kind.
    """
    summaries = (row or {}).get("storms") or []
    grouped: dict = {}
    for index, path in enumerate(_ordered_round_files(outdir, "storm-round", ".tsv")):
        storm = summaries[index] if index < len(summaries) else None
        if storm is not None:
            background, kind = storm_background(storm), storm_kind(storm)
        else:
            background = (
                "idle" if isinstance(profile, str) and profile.startswith("storm-idle") else "scan"
            )
            kind = storm_kind({"profile": profile or ""})
        found = []
        try:
            text = path.read_text()
        except OSError:
            continue
        for line in text.splitlines():
            parts = line.split("\t")
            if len(parts) < 4:
                continue
            try:
                found.append(float(parts[2]))  # the TSV records seconds
            except ValueError:
                continue
        if found:
            values, count = grouped.setdefault(f"{background}_storm_{kind}", ([], 0))
            values.extend(found)
            grouped[f"{background}_storm_{kind}"] = (values, count + 1)
    return grouped


def _storm_raw_from_events(events) -> dict:
    grouped: dict = {}
    for sample in events.storm_samples:
        value = sample.get("latency_ms")
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            continue
        key = f"{storm_background(sample)}_storm_{storm_kind(sample)}"
        grouped.setdefault(key, ([], 0))[0].append(round(float(value) / 1000.0, 9))
    for summary in events.storm_summaries:
        key = f"{storm_background(summary)}_storm_{storm_kind(summary)}"
        values, count = grouped.get(key, ([], 0))
        grouped[key] = (values, count + 1)
    return grouped


def _profile_clients(profile, row: dict) -> int:
    """Resolved client count for a storm profile; 1 for anything else."""
    try:
        ncpu = int(row.get("cpu") or 8)
    except TypeError, ValueError:
        ncpu = 8
    derived = config.storm_profile_clients(profile, ncpu)
    return derived if derived is not None else 1


def _collect_raw_samples(row: dict, outdir: Path, profile, events=None) -> None:
    """Store every raw measurement (seconds) and its source-cluster count.

    Artifacts hold the individual timed runs, fetches and storm ops; the
    event stream holds the same values when it is present. Both paths end
    up in the same two helper mappings, keyed by probe/scenario name.
    """
    samples: dict = {}
    counts: dict = {}

    def add(key: str, values: list, clusters: int) -> None:
        if values:
            samples[key] = list(values)
            counts[key] = int(clusters)

    post = hyperfine_runs(outdir / "hyperfine.json")
    for access, bench in _POST_SCAN_BENCH.items():
        values = [float(value) for value in post.get(bench, [])]
        if not values:
            recorded = row.get(_POST_SCAN_FIELDS[access])
            values = [float(recorded)] if recorded is not None else []
        add(f"idle_{access}", values, len(values))

    bucket = "scan"
    round_values, round_counts = _round_hyperfine_runs(outdir)
    for access in ("prefetch", "edit"):
        values = round_values[access]
        clusters = round_counts[access]
        if not values:
            values = [float(value) for value in row.get(_PROBE_ROUND_FIELDS[access]) or []]
            clusters = len(values)
        add(f"{bucket}_{access}", values, clusters)
    thumb_values, thumb_count = _round_fetch_samples(outdir)
    if not thumb_values:
        thumb_values = [float(value) for value in row.get(_PROBE_ROUND_FIELDS["thumb"]) or []]
        thumb_count = len(thumb_values)
    add(f"{bucket}_thumb", thumb_values, thumb_count)

    storm = {}
    if events is not None and events.storm_samples:
        storm = _storm_raw_from_events(events)
    if not storm:
        storm = _storm_raw_from_artifacts(outdir, profile, row)
    for key, (values, clusters) in storm.items():
        add(key, values, clusters)

    if events is not None and events.upload_files:
        submit = [float(event["submit_ms"]) / 1000.0 for event in events.upload_files]
        visible = [float(event["visible_ms"]) / 1000.0 for event in events.upload_files]
        upload_count = len(events.upload_files)
    else:
        try:
            log = (outdir / "harness-a.log").read_text()
        except OSError:
            log = ""
        pairs = _UPLOAD_FILE_RE.findall(log)
        submit = [float(raw) / 1000.0 for raw, _visible in pairs]
        visible = [float(raw) / 1000.0 for _submit, raw in pairs]
        upload_count = len(pairs)
    add("upload_submit", submit, upload_count)
    add("upload_visible", visible, upload_count)

    walls = [float(value) for value in row.get("scan_runs_ms") or []]
    if not walls:
        walls = [
            float(part)
            for part in str(row.get("scan_runs_s", "")).split(",")
            if part and part != "n/a"
        ]
    add("scan_wall", walls, len(walls))

    # Stage samples: one per scan per stage, skip n == 0.
    stage_values: dict = {}
    for stage in row.get("drain_stages") or row.get("stages") or []:
        if int(stage.get("n", 0) or 0) == 0:
            continue
        value = stage.get("p50_us")
        if value is None:
            continue
        stage_values.setdefault(stage.get("stage"), []).append(float(value))
    for name, vals in stage_values.items():
        add(f"scan_stage_{name}", vals, len(vals))

    # Gauge samples: one value per rep.
    for column in METRICS_COLUMNS:
        value = row.get(column)
        if value is None:
            continue
        try:
            add(f"idle_gauge_{column}", [float(value)], 1)
        except TypeError, ValueError:
            continue

    # Count samples: failed ops per storm run, keyed by the access it drove.
    for storm in row.get("storms") or []:
        bg = storm_background(storm)
        access = {"read": "prefetch", "thumb": "thumb", "edit": "edit"}.get(
            storm_kind(storm), storm_kind(storm)
        )
        add(f"{bg}_count_{access}", [float(storm.get("failed", 0) or 0)], 1)

    # Scroll samples split by background.
    scroll_by_bg = row.get("scroll_samples_by_bg") or {}
    for bg in ("idle", "scan"):
        bucket = scroll_by_bg.get(bg) or {}
        for src, dst in (
            ("scrollbar", "scroll_bar"),
            ("rows", "scroll_rows"),
            ("data", "scroll_data"),
        ):
            vals = bucket.get(src) or []
            if vals:
                add(f"{bg}_{dst}", [float(v) / 1000.0 for v in vals], len(vals))

    def _raw_to_metric(raw_key):
        if raw_key.startswith("idle_") or raw_key.startswith("scan_"):
            bg = raw_key.split("_", 1)[0]
            rest = raw_key.split("_", 1)[1]
            if rest.startswith("storm_"):
                parts = rest.split("_", 2)
                kind = parts[1] if len(parts) > 1 else ""
                access = {"read": "prefetch", "thumb": "thumb", "edit": "edit"}.get(kind, kind)
                return ("latency", bg, access)
            if rest.startswith("count_"):
                return ("count", bg, rest[len("count_") :])
            if rest.startswith("stage_"):
                return ("stage", bg, rest[len("stage_") :])
            if rest.startswith("gauge_"):
                return ("gauge", bg, rest[len("gauge_") :])
            if rest == "wall":
                return ("wall", "scan", "scan")
            if rest in ACCESSES:
                return ("latency", bg, rest)
            return None
        if raw_key.startswith("upload_"):
            subject = raw_key[len("upload_") :]
            return ("upload", "idle", subject)
        return None

    metric_samples = {}
    metric_clusters = {}
    for key, values in samples.items():
        info = _raw_to_metric(key)
        if info is None:
            continue
        family, bg, subject = info
        mkey = metrics.metric_key(family, bg, subject)
        if family in ("latency", "upload"):
            mvalues = [float(v) * 1000.0 for v in values]
        else:
            mvalues = [float(v) for v in values]
        metric_samples[mkey] = mvalues
        metric_clusters[mkey] = max(1, int(counts.get(key, len(values))))

    row["raw_samples"] = samples
    row["raw_cluster_counts"] = counts
    row["samples"] = metric_samples
    row["clusters"] = metric_clusters


def parse_log(outdir: Path, profile=None) -> dict:
    """Read one cell dir into a row, preferring its `events.jsonl` stream.

    ``profile`` is the cell's WORKLOAD profile. It labels the storm access
    and background and resolves the client count; it is the battery's label
    for the cell, not something the artifacts are searched for.
    """
    log = (outdir / "harness-a.log").read_text()
    row: dict = {"benchmark": outdir.name}
    _apply_setup(row, log)
    events = read_cell_events(outdir, log)
    if events is None:
        _apply_artifact_measurements(row, outdir, log)
        for column in (
            "probe_rounds_total",
            "probe_rounds_valid",
            "probe_coverage_pct",
            "warmup_valid",
            "phase_invalid",
            "phase_invalid_reasons",
        ):
            row.setdefault(column, "n/a")
            row.setdefault("storm_coverage_pct", "n/a")
    else:
        _apply_events(row, events, log)
    _apply_post_scan(row, log, outdir)
    _apply_stage_rows(row, outdir, log)
    _apply_cell_metrics(row, outdir)
    _collect_raw_samples(row, outdir, profile, events)

    # Per-rep record fields.
    manifest = {}
    try:
        manifest = json.loads((outdir / "run-manifest.json").read_text())
    except OSError, json.JSONDecodeError:
        manifest = {}
    upload_files = None
    raw_counts = row.get("raw_cluster_counts") or {}
    if raw_counts.get("upload_submit") is not None:
        upload_files = raw_counts.get("upload_submit")
    row["identity"] = {
        "source": "battery",
        "profile": profile,
        "images": row.get("images"),
        "upload_files": upload_files,
        "build": manifest.get("build_profile"),
        "commit": manifest.get("git"),
        "scan_runs": row.get("scan_runs"),
    }
    ops_done = 0
    ops_planned = 0
    failed_count = 0
    for storm in row.get("storms") or []:
        ops_done += int(storm.get("done", 0) or 0)
        ops_planned += int(storm.get("total", 0) or 0)
        failed_count += int(storm.get("failed", 0) or 0)
    row["validity"] = {
        "probe_rounds_total": row.get("probe_rounds_total", "n/a"),
        "probe_rounds_valid": row.get("probe_rounds_valid", "n/a"),
        "probe_coverage_pct": row.get("probe_coverage_pct", "n/a"),
        "storm_coverage_pct": row.get("storm_coverage_pct", "n/a"),
        "warmup_valid": row.get("warmup_valid", "n/a"),
        "phase_invalid": row.get("phase_invalid", "n/a"),
        "phase_invalid_reasons": row.get("phase_invalid_reasons", "n/a"),
        "ops_done": ops_done if (row.get("storms") or []) else "n/a",
        "ops_planned": ops_planned if (row.get("storms") or []) else "n/a",
        "failed": failed_count if (row.get("storms") or []) else "n/a",
    }
    if "samples" not in row or not isinstance(row.get("samples"), dict):
        row["samples"] = {}
    if "clusters" not in row or not isinstance(row.get("clusters"), dict):
        row["clusters"] = {}
    row.setdefault("cell", outdir.name)
    if isinstance(row.get("identity"), dict):
        row["identity"].setdefault("cell", row.get("cell"))
        row["identity"].setdefault("scan_runs", row.get("scan_runs"))
    return row


def parse_status(outdir: Path) -> dict:
    try:
        return json.loads((outdir / "status-final.json").read_text())
    except (OSError, json.JSONDecodeError) as exc:
        print(f"error: {outdir}: cannot read status-final.json: {exc}", file=sys.stderr)
        sys.exit(1)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("outdirs", nargs="+", help="bench OUT dirs")
    parser.add_argument(
        "--profile",
        help="WORKLOAD profile of the cell(s)",
    )
    args = parser.parse_args(argv)

    for raw in args.outdirs:
        outdir = Path(raw)
        if not (outdir / "harness-a.log").exists():
            print(f"error: {outdir}: no harness-a.log", file=sys.stderr)
            return 1
        row = parse_log(outdir, profile=args.profile)
        identity = row.get("identity") or {}
        validity = row.get("validity") or {}
        print(f"cell: {row.get('cell', outdir.name)}")
        print(f"identity: {json.dumps(identity, sort_keys=True)}")
        print(f"validity: {json.dumps(validity, sort_keys=True)}")
        samples = row.get("samples") or {}
        clusters = row.get("clusters") or {}
        for key in sorted(samples):
            values = samples[key]
            family, background, subject = metrics.split_key(key)
            summary = metrics.summarize(values, family)
            stat_names = metrics.statistics_for(family)
            parts = []
            for stat in stat_names:
                if stat == metrics.COUNT_STAT:
                    parts.append(f"sum={int(round(float(summary[stat])))}")
                else:
                    parts.append(f"{stat}={float(summary[stat]):.1f}")
            ci = ""
            if "p50" in summary:
                low, high = metrics.bootstrap_interval(values)
                if low is not None and high is not None:
                    ci = f" ci={float(low):.1f}-{float(high):.1f}"
            print(
                f"  {key}: " + " ".join(parts) + f" n={len(values)} "
                f"clusters={clusters.get(key, len(values))}" + ci
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
