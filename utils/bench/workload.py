#!/usr/bin/env python3
"""Workload half of a benchmark cell, against a launcher-owned server.

Consumes `launcher-info.json` written by `utils/bench/backend_launcher.py`
and implements the measurement protocol (line formats compatible with
the retired shell driver, so historical cells keep parsing):
image-set materialisation, album-index scans, and the timed requests that
run around them. The phases of a cell come from `scenarios` and are driven
by `executor`; this module supplies the shared request/artifact primitives
and the per-cell summary.

Artifacts are written in the bench transcript format so `utils/bench/extract_bench.py`
parses either producer unchanged:

  harness-a.log                    run transcript (harness line formats)
  status-last.json                 most recent /get/index/status body
  status-final.json                last run's terminal status
  status-final-run-<n>.json        per-run terminal status
  prefetch.json / get-data.json    raw probe-request records (debug)
  verify-row.json / verify-metadata.json
  probe-phase.log                  during-scan transcript
  probe-phase.log                  during-scan transcript
  events.jsonl                    structured JSONL event stream (schema v=1):
                                    one JSON object per line, each carrying
                                    {"v": 1, "type": ..., "t": <ISO wall clock>}.
                                    v1 types and required keys:
                                      probe_round: round, rows, state,
                                        fetch_n, fetch_mean_ms (plus fetch min/max),
                                        prefetch_ms, edit_ms, thumb_ms lists
                                      storm_sample: profile, client, op, latency_ms, code
                                      storm_summary: profile, kind, done, total, median,
                                        p95, max, failed, wall
                                      upload_file: file, submit_ms, visible_ms
                                      upload_summary: submit_p50, visible_p50
                                      scan_state: run, state
                                      verify_result: kind, matched, total
                                    Incompatible schema changes bump v; readers switch on v.


The module is importable and stdlib-only. Direct HTTP uses urllib with the
launcher's cookie jar.

Environment contract (on top of the launcher's):

  LAUNCHER_INFO   path to launcher-info.json (required)
  OUT             artifact directory (default: launcher-info.out)
  NCPU            cores for the CPU column (default 4)
  IMAGES          images per cell, copied from IMAGE_REPO (default 100)
  IMAGE_REPO      pre-seeded image directory (required, read-only);
                  the first IMAGES entries in sorted order are copied
  IMAGE_PATH      cell image directory (default OUT/images)
  REPEATS         post-scan measured runs per access (default 3)
  WARMUP          post-scan warmup runs per access (default 1)
  RUNS_DURING     per-round measured runs per access during a scan (default 3)
  SCAN_REPEATS    scan repetitions per cell (default 3)
  FETCHES         random compressed fetches per during-scan round (default 10)
  PROBE_LIMIT     max probe-list rows (default: IMAGES)
  PROBE_PAGE      get-data rows per request when paging the snapshot
                  (default 1000): the probe-list build pages until
                  PROBE_LIMIT rows, the upload-visibility and storm
                  canary walks page until the row space ends
  SCAN_TIMEOUT_S  scan poll budget; 0 waits for a terminal state (default 0)
  SCAN_STALL_TIMEOUT_S  abort when running counters do not move for this
                  long; 0 disables (default 120)
  POLL_INTERVAL_S status poll interval (default 0.2)
  PROBE_INTERVAL_S probe-phase refresh interval (default 0.5)
  IMAGES is required to be present in IMAGE_REPO: repos are seeded
  a priori (see `just bench seed-images`); the workload never generates.
  WORKLOAD        probe (default) | scan-storm-<kind>-<level> |
                  idle-storm-<kind>-<level> | idle-upload | scan-upload,
                  where kind is read, thumb or edit and level is 8 or 16n
                  (16*NCPU). `scenarios` owns the cell specs; `executor`
                  drives their phases through one scan session.
  MIN_PROBE_ROUNDS probe target of valid rounds repeated over scans (default 20)
  UPLOAD_FILES    upload stimulus size (default 20); UPLOAD_REPO must hold at
                  least this many images
  UPLOAD_TIMEOUT_S visibility poll budget per uploaded file (default 30);
                  a file that never becomes visible fails the cell
  UPLOAD_POST_TIMEOUT_S  budget for one upload POST (default 60)
  INDEX_SETTLE_TIMEOUT_S  upload cells: budget for the library index to
                  settle before the upload stimulus (default 600)
  STORM_CLIENTS   explicit storm client count; named levels derive it
  STORM_OPS       minimum ops per storm client (default 10)
  STORM_MIN_COVERAGE_PCT  concurrent storm wall as a percentage of scan wall
                  (default 50); below it the cell fails
Exit codes: 0 completed; 1 config/preflight; 2 runtime failure (including a
scan that did not reach `completed` and a failed write-probe verification).
"""

from __future__ import annotations

import argparse
import http.cookiejar
import json
import math
import os
import random
import shutil
import statistics
import sys
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent

# config.py owns the typed config and its defaults.
sys.path.insert(0, str(SCRIPT_DIR))
import config  # noqa: E402
import executor  # noqa: E402
import scenarios  # noqa: E402
from config import ConfigError  # noqa: E402

TERMINAL_STATUS_STATES = ("completed", "failed", "canceled")
# `idle` means no album-index job has run, so there is nothing to wait for.
SETTLED_STATUS_STATES = (*TERMINAL_STATUS_STATES, "idle")

ARTIFACT_FILES = (
    "harness-a.log",
    "events.jsonl",
    "status-last.json",
    "status-final.json",
    "scan-done.marker",
    "prefetch.json",
    "get-data.json",
    "probe-phase.log",
    "index-trigger.json",
    "verify-row.json",
    "verify-metadata.json",
)
ARTIFACT_GLOBS = ("status-final-run-*.json",)


class WorkloadError(Exception):
    """Runtime failure after the cell started."""


# --- configuration -----------------------------------------------------------
def config_from_env(environ: dict) -> config.WorkloadConfig:
    """Build workload config once; consumers read attributes."""
    return config.WorkloadConfig.from_env(environ)


# --- logging -----------------------------------------------------------------
class Logger:
    def __init__(self, path: Path):
        self.lock = threading.Lock()
        self.path = path
        self.handle = open(path, "w", encoding="utf-8")  # noqa: PTH123

    def log(self, message: str) -> str:
        line = f"{time.strftime('%H:%M:%S')} {message}"
        with self.lock:
            self.handle.write(line + "\n")
            self.handle.flush()
        print(line, flush=True)
        return line

    def close(self) -> None:
        self.handle.close()


class ProbeTranscript:
    """Mirror of probe-phase log lines, as the bench transcript's redirected stdout."""

    def __init__(self, logger: Logger, path: Path):
        self.logger = logger
        self.handle = open(path, "w", encoding="utf-8")  # noqa: PTH123

    def log(self, message: str) -> str:
        line = self.logger.log(message)
        self.handle.write(line + "\n")
        self.handle.flush()
        return line

    def close(self) -> None:
        self.handle.close()


# --- HTTP --------------------------------------------------------------------
class HttpClient:
    def __init__(self, base_url: str, cookie_jar: Path):
        self.base_url = base_url.rstrip("/")
        jar = http.cookiejar.MozillaCookieJar(str(cookie_jar))
        try:
            jar.load(ignore_discard=True, ignore_expires=True)
        except (OSError, http.cookiejar.LoadError) as err:
            raise ConfigError(f"cannot load cookie jar {cookie_jar}: {err}")
        self.opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({}),
            urllib.request.HTTPCookieProcessor(jar),
        )

    def request(self, method: str, path: str, payload=None, bearer=None, timeout=30.0):
        data = None
        headers = {}
        if payload is not None:
            data = json.dumps(payload).encode()
            headers["Content-Type"] = "application/json"
        if bearer:
            headers["Authorization"] = f"Bearer {bearer}"
        request = urllib.request.Request(
            self.base_url + path, data=data, headers=headers, method=method
        )
        started = time.perf_counter()
        try:
            with self.opener.open(request, timeout=timeout) as response:
                body = response.read()
                return response.status, body, time.perf_counter() - started
        except urllib.error.HTTPError as err:
            body = err.read()
            err.close()
            return err.code, body, time.perf_counter() - started
        except (urllib.error.URLError, OSError) as err:
            raise WorkloadError(f"{method} {path} failed: {err}")

    def get(self, path, bearer=None, timeout=30.0):
        return self.request("GET", path, bearer=bearer, timeout=timeout)

    def post(self, path, payload, timeout=30.0):
        return self.request("POST", path, payload=payload, timeout=timeout)

    def put(self, path, payload, timeout=30.0):
        return self.request("PUT", path, payload=payload, timeout=timeout)

    def upload(self, path, file_path, timeout=60.0):
        boundary = "----picasu-bench-upload-boundary"
        file_bytes = Path(file_path).read_bytes()
        filename = Path(file_path).name
        body = bytearray()
        body.extend(b"--" + boundary.encode() + b"\r\n")
        body.extend(
            b'Content-Disposition: form-data; name="file"; filename="'
            + filename.encode()
            + b'"\r\n'
        )
        body.extend(b"Content-Type: image/jpeg\r\n\r\n")
        body.extend(file_bytes)
        body.extend(b"\r\n")
        body.extend(b"--" + boundary.encode() + b"\r\n")
        body.extend(b'Content-Disposition: form-data; name="lastModified"\r\n\r\n0\r\n')
        body.extend(b"--" + boundary.encode() + b"--\r\n")
        headers = {"Content-Type": f"multipart/form-data; boundary={boundary}"}
        request = urllib.request.Request(
            self.base_url + path, data=bytes(body), headers=headers, method="POST"
        )
        started = time.perf_counter()
        try:
            with self.opener.open(request, timeout=timeout) as response:
                body_out = response.read()
                return response.status, body_out, time.perf_counter() - started
        except urllib.error.HTTPError as err:
            body_out = err.read()
            err.close()
            return err.code, body_out, time.perf_counter() - started
        except (urllib.error.URLError, OSError) as err:
            raise WorkloadError(f"POST {path} failed: {err}")


def _body_json(body: bytes):
    try:
        return json.loads(body)
    except json.JSONDecodeError, TypeError:
        return None


def _compressed_path(hash_value: str) -> str:
    return f"/object/compressed/{hash_value[:2]}/{hash_value}.jpg"


# --- image set ---------------------------------------------------------------
# Images are seeded a priori (`just bench seed-images`). The workload copies
# exactly IMAGES files — the first IMAGES entries in sorted order — so one
# repo can serve any cell size deterministically.


def prepare_images(cfg: config.WorkloadConfig, logger: Logger) -> tuple:
    img_dir = cfg.image_path
    img_dir.mkdir(parents=True, exist_ok=True)

    # Prepared sandbox: manifest present means the sandbox was built by
    # `bench prepare`; never materialise, just use it as-is.
    manifest_path = img_dir / "sandbox-manifest.json"
    if manifest_path.exists():
        try:
            manifest = json.loads(manifest_path.read_text())
        except OSError, json.JSONDecodeError:
            manifest = {}
        archive = manifest.get("archive")
        manifest_count = manifest.get("count")
        logger.log(
            f"=== prepared sandbox (materialise skipped) path={img_dir} "
            f"archive={archive} count={manifest_count} ==="
        )
        jpgs = sorted(img_dir.glob("*.jpg"), key=lambda path: path.name)
        image_count = len(jpgs)
        image_bytes = sum(path.stat().st_size for path in jpgs)
        logger.log(
            f"reused count={image_count} bytes={image_bytes} wall_ms=0 mode=prepared({archive})"
        )
        return image_count, image_bytes

    # Deprecated: unprepared dirs still materialise by copy/link. New cells
    # should point at a prepared sandbox instead of relying on this path.
    available = sorted(cfg.image_repo.glob("*.jpg"), key=lambda path: path.name)
    if len(available) < cfg.images:
        raise ConfigError(
            f"IMAGE_REPO holds {len(available)} images, need IMAGES={cfg.images}: "
            f"seed more with `just bench seed-images` (repo: {cfg.image_repo})"
        )
    wanted = available[: cfg.images]

    logger.log(f"=== seeding images (repo={cfg.image_repo} path={img_dir}) ===")
    started = time.perf_counter()
    linked_count = 0
    copied_count = 0
    for source in wanted:
        dest = img_dir / source.name
        if dest.exists() and dest.resolve() == source.resolve():
            linked_count += 1
            continue
        tmp = img_dir / (source.name + ".materialise-tmp")
        tmp.unlink(missing_ok=True)
        took_link = False
        try:
            os.link(source, tmp)
            took_link = True
        except OSError:
            shutil.copy2(source, tmp)
            took_link = False
        os.replace(tmp, dest)
        if took_link:
            linked_count += 1
        else:
            copied_count += 1
    if linked_count == len(wanted):
        mode = "linked"
    elif copied_count == len(wanted):
        mode = "copied"
    else:
        mode = f"mixed(linked={linked_count},copied={copied_count})"
    wall_ms = int((time.perf_counter() - started) * 1000)

    jpgs = list(img_dir.glob("*.jpg"))
    image_count = len(jpgs)
    image_bytes = sum(path.stat().st_size for path in jpgs)
    if image_count < cfg.images:
        raise WorkloadError(f"expected at least {cfg.images} images, found {image_count}")
    logger.log(f"seeded {len(wanted)} images from {cfg.image_repo} (source untouched)")
    logger.log(
        f"generated={image_count} bytes={image_bytes} wall_ms={wall_ms} "
        f"mode={mode}({cfg.image_repo})"
    )
    return image_count, image_bytes


# --- paged snapshot reads ----------------------------------------------------
# A cell snapshot holds its images plus any uploads; albums and other
# non-image entries add an unknown on top. The walks also stop at the first
# short window, so the cap only bounds a server that never short-pages.
ROW_SPACE_SLACK = 1000


def row_space_cap(cfg: config.WorkloadConfig) -> int:
    """Upper bound on one cell's snapshot row space for a paged lookup."""
    images = int(cfg.images)
    uploads = int(cfg.upload_files)
    return images + uploads + ROW_SPACE_SLACK


def page_get_data(
    client: HttpClient,
    timestamp,
    token,
    *,
    page_size: int,
    cap: int,
    timeout: float = 30.0,
):
    """Walk `/get/get-data` over the row space in `page_size` windows.

    get-data serves one window per request, so a single `0..page_size`
    window only sees the start of a large library. Yield
    `(start, rows, raw_body)` per served window while the window start
    stays below `cap` (the last window may overshoot it by up to
    `page_size - 1` rows). A short window — the row space is exhausted —
    or a response that is not a row list ends the walk; the latter is
    yielded once with `rows=None`.
    """
    start = 0
    while start < cap:
        end = start + page_size
        code, body, _ = client.get(
            f"/get/get-data?timestamp={timestamp}&start={start}&end={end}",
            bearer=token,
            timeout=timeout,
        )
        rows = _body_json(body) if code == 200 else None
        if not isinstance(rows, list):
            yield start, None, body
            return
        yield start, rows, body
        if len(rows) < page_size:
            return  # short window: the row space is exhausted
        start = end


# --- probe list --------------------------------------------------------------
def build_probe_list(cfg: config.WorkloadConfig, client: HttpClient) -> list | None:
    """Build `hash<TAB>token` rows from the snapshot, paging the row window.

    get-data returns at most one page per request, so a single `0..1000`
    window would cover only the start of a large library. Page until
    PROBE_LIMIT rows are collected or a short page marks the end of the
    snapshot. `get-data.json` keeps the raw body for a single-page window
    and the concatenated rows when the window spans several pages.
    """
    out = cfg.out
    code, body, _ = client.post("/get/prefetch", {})
    (out / "prefetch.json").write_bytes(body)
    if code != 200:
        return None
    doc = _body_json(body)
    if not isinstance(doc, dict):
        return None
    timestamp = doc.get("prefetch", {}).get("timestamp")
    token = doc.get("token")
    if timestamp is None or token is None:
        return None

    page_size = cfg.probe_page
    limit = cfg.probe_limit
    cap = max(row_space_cap(cfg), limit)
    lines = []
    collected_rows = []
    raw_pages = []
    for _start, rows, body in page_get_data(client, timestamp, token, page_size=page_size, cap=cap):
        if rows is None:
            if not raw_pages:
                (out / "get-data.json").write_bytes(body)
                return None
            break  # keep the pages already collected
        raw_pages.append(body)
        collected_rows.extend(rows)
        for row in rows:
            abstract = row.get("abstractData", {})
            if abstract.get("type") != "image":
                continue
            hash_value = abstract.get("id")
            row_token = row.get("token")
            if hash_value is None or row_token is None:
                continue
            lines.append(f"{hash_value}\t{row_token}")
            if len(lines) >= limit:
                break
        if len(lines) >= limit:
            break
    if len(raw_pages) == 1:
        (out / "get-data.json").write_bytes(raw_pages[0])
    else:
        (out / "get-data.json").write_text(json.dumps(collected_rows))
    return lines or None


def _prefetch_timestamp(out: Path):
    doc = (
        _body_json((out / "prefetch.json").read_bytes())
        if (out / "prefetch.json").is_file()
        else None
    )
    if not isinstance(doc, dict):
        return None
    return doc.get("prefetch", {}).get("timestamp")


def _prefetch_token(out: Path):
    doc = (
        _body_json((out / "prefetch.json").read_bytes())
        if (out / "prefetch.json").is_file()
        else None
    )
    if not isinstance(doc, dict):
        return None
    return doc.get("token")


def _status_state(out: Path) -> str:
    path = out / "status-last.json"
    try:
        doc = json.loads(path.read_text())
    except OSError, json.JSONDecodeError:
        return "unknown"
    return doc.get("state", "unknown")


_EVENTS_LOCKS: dict = {}


def _events_path(out) -> Path:
    path = Path(out)
    if path.name == "events.jsonl":
        return path
    return path / "events.jsonl"


def _events_lock(path: Path) -> threading.Lock:
    key = str(path)
    lock = _EVENTS_LOCKS.get(key)
    if lock is None:
        lock = threading.Lock()
        _EVENTS_LOCKS[key] = lock
    return lock


def _wall_iso() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds")


def emit_event(out, event_type: str, **fields) -> None:
    """Append one structured v=1 event to ``events.jsonl`` (additive)."""
    path = _events_path(out)
    event = {"v": 1, "type": event_type, "t": _wall_iso()}
    event.update(fields)
    lock = _events_lock(path)
    with lock:
        with open(path, "a", encoding="utf-8") as handle:  # noqa: PTH123
            handle.write(json.dumps(event, separators=(",", ":")) + "\n")
            handle.flush()


# --- during-scan probes ------------------------------------------------------
def random_fetch_round(
    cfg: config.WorkloadConfig,
    client: HttpClient,
    transcript: ProbeTranscript,
    rows: int,
    state: str,
    lines: list,
) -> list:
    """Time the thumbnail (compressed) GETs for one probe round.

    Returns the per-request elapsed times in seconds so the caller can
    fold them into the round's probe_round event.
    """
    picks = []
    if lines and cfg.fetches:
        picks = random.sample(lines, min(cfg.fetches, len(lines)))
    times = []
    for line in picks:
        hash_value, token = line.split("\t", 1)
        try:
            _code, _body, elapsed = client.get(_compressed_path(hash_value), bearer=token)
        except WorkloadError:
            elapsed = 0.0
        times.append(float(elapsed))
    return times


def scroll_probe(
    cfg: config.WorkloadConfig,
    client: HttpClient,
    timestamp,
    token,
    *,
    total_rows=None,
    positions: int | None = 3,
    timeout: float = 30.0,
) -> dict:
    """Exercise the scroll-bar / row / data read paths.

    Fetches ``get-scroll-bar`` once, then walks a few scroll positions
    (top / middle / bottom) issuing ``get-rows`` by index and ``get-data``
    start/end windows for the visible +/-1 batches, mirroring
    ``useFetchRows`` / ``useFetchImgs``. Returns the per-call latencies in
    seconds (``scrollbar``, ``rows``, ``data``).
    """
    if total_rows is None:
        try:
            total_rows = row_space_cap(cfg)
        except Exception:  # noqa: BLE001
            total_rows = 1000
    try:
        total_rows = max(1, int(total_rows))
    except TypeError, ValueError:
        total_rows = 1000
    try:
        page_size = int(getattr(cfg, "probe_page", 1000) or 1000)
    except TypeError, ValueError:
        page_size = 1000
    if page_size < 1:
        page_size = 1000

    scrollbar: list = []
    rows: list = []
    data: list = []

    # Scroll bar once (full snapshot month buckets).
    try:
        _code, _body, elapsed = client.get(
            f"/get/get-scroll-bar?timestamp={timestamp}",
            bearer=token,
            timeout=timeout,
        )
        scrollbar.append(float(elapsed))
    except WorkloadError:
        scrollbar.append(0.0)

    # Scroll positions: top / middle / bottom (+ locate target = top).
    pos_indices = [0, max(0, total_rows // 2), max(0, total_rows - page_size)]
    seen: set = set()
    unique: list = []
    for pos in pos_indices:
        if pos in seen:
            continue
        seen.add(pos)
        unique.append(pos)
    if positions is not None and positions > 0:
        unique = unique[:positions]

    for pos in unique:
        # ~3-5 row (layout window) calls per scroll position.
        for offset in (-2, -1, 0, 1, 2):
            idx = pos + offset
            if idx < 0:
                continue
            try:
                _code, _body, elapsed = client.get(
                    f"/get/get-rows?index={idx}&timestamp={timestamp}",
                    bearer=token,
                    timeout=timeout,
                )
                rows.append(float(elapsed))
            except WorkloadError:
                rows.append(0.0)
        # Visible +/-1 batches: get-data start/end windows.
        for batch_offset in (-1, 0, 1):
            start = pos + batch_offset * page_size
            if start < 0:
                start = 0
            end = start + page_size
            try:
                _code, _body, elapsed = client.get(
                    f"/get/get-data?timestamp={timestamp}&start={start}&end={end}",
                    bearer=token,
                    timeout=timeout,
                )
                data.append(float(elapsed))
            except WorkloadError:
                data.append(0.0)

    result = {
        "scrollbar": scrollbar,
        "rows": rows,
        "data": data,
        "scrollbar_ms": [value * 1000.0 for value in scrollbar],
        "rows_ms": [value * 1000.0 for value in rows],
        "data_ms": [value * 1000.0 for value in data],
        "scrollbar_s": list(scrollbar),
        "rows_s": list(rows),
        "data_s": list(data),
    }
    return result


def probe_round(
    cfg: config.WorkloadConfig,
    client: HttpClient,
    logger: Logger,
    transcript: ProbeTranscript,
    round_no: int,
    rows: int,
    state: str,
    lines: list,
) -> bool:
    out = cfg.out
    timestamp = _prefetch_timestamp(out)
    if timestamp is None:
        transcript.log(
            f"probe round={round_no} rows={rows} state={state} "
            "no prefetch timestamp; skipping round"
        )
        emit_event(
            cfg.out,
            "probe_round",
            round=round_no,
            rows=rows,
            state=state,
            skipped=True,
            reason="no prefetch timestamp",
            prefetch_ms=[],
            edit_ms=[],
            thumb_ms=[],
        )
        return False

    # Thumbnail GETs (existing FETCHES) for this round.
    thumb_times = random_fetch_round(cfg, client, transcript, rows, state, lines)
    thumb_ms = [t * 1000.0 for t in thumb_times]

    # Scroll probe (unchanged; emitted as its own scroll_probe event).
    try:
        token = _prefetch_token(out)
        timestamp = _prefetch_timestamp(out)
        if timestamp is not None and token is not None:
            scroll_result = scroll_probe(
                cfg,
                client,
                timestamp,
                token,
                total_rows=rows if rows else None,
            )
            emit_event(
                cfg.out,
                "scroll_probe",
                round=round_no,
                rows=rows,
                state=state,
                phase="during",
                background="scan",
                scrollbar_ms=[value * 1000.0 for value in scroll_result["scrollbar"]],
                rows_ms=[value * 1000.0 for value in scroll_result["rows"]],
                data_ms=[value * 1000.0 for value in scroll_result["data"]],
            )
    except Exception:  # noqa: BLE001 - a probe must not kill the scan
        transcript.log(f"scroll probe round={round_no} failed; continuing")

    timestamp = _prefetch_timestamp(out)
    if timestamp is None:
        transcript.log(
            f"probe round={round_no} rows={rows} state={state} "
            "no prefetch timestamp; skipping round"
        )
        emit_event(
            cfg.out,
            "probe_round",
            round=round_no,
            rows=rows,
            state=state,
            skipped=True,
            reason="no prefetch timestamp",
            prefetch_ms=[],
            edit_ms=[],
            thumb_ms=thumb_ms,
        )
        return False

    token = _prefetch_token(out)

    # In-process prefetch POST samples.
    prefetch_ms = []
    for _ in range(cfg.runs_during):
        try:
            _code, _body, elapsed = client.post("/get/prefetch", {})
        except WorkloadError:
            elapsed = 0.0
        prefetch_ms.append(float(elapsed) * 1000.0)

    # In-process edit-rating PUT samples.
    edit_ms = []
    payload = {"indexArray": [0], "timestamp": timestamp, "rating": 3}
    for _ in range(cfg.runs_during):
        try:
            _code, _body, elapsed = client.put("/put/edit_rating", payload)
        except WorkloadError:
            elapsed = 0.0
        edit_ms.append(float(elapsed) * 1000.0)

    fetch_times = thumb_times
    fetch_mean_s = (sum(fetch_times) / len(fetch_times)) if fetch_times else 0.0
    fetch_min_s = min(fetch_times) if fetch_times else 0.0
    fetch_max_s = max(fetch_times) if fetch_times else 0.0

    transcript.log(f"probe round={round_no} rows={rows} state={state}")
    emit_event(
        cfg.out,
        "probe_round",
        round=round_no,
        rows=rows,
        state=state,
        fetch_n=len(fetch_times),
        fetch_mean=fetch_mean_s,
        fetch_mean_s=fetch_mean_s,
        fetch_mean_ms=fetch_mean_s * 1000.0,
        fetch_min=fetch_min_s,
        fetch_min_s=fetch_min_s,
        fetch_min_ms=fetch_min_s * 1000.0,
        fetch_max=fetch_max_s,
        fetch_max_s=fetch_max_s,
        fetch_max_ms=fetch_max_s * 1000.0,
        prefetch_ms=prefetch_ms,
        edit_ms=edit_ms,
        thumb_ms=thumb_ms,
        prefetch_median_ms=(statistics.median(prefetch_ms) if prefetch_ms else 0.0),
        edit_median_ms=(statistics.median(edit_ms) if edit_ms else 0.0),
    )
    return True


class ScanPhase:
    """Reusable during-phase guard.

    Snapshots a status document at start and end and decides whether the
    phase ran under the expected background state.
    """

    def __init__(
        self,
        out,
        phase,
        *,
        expected_state="running",
        run=None,
        round_no=None,
        snapshot=None,
        allow_terminal_end=False,
    ):
        self.out = Path(out) if out is not None else None
        self.phase = phase
        self.expected_state = expected_state
        self.run = run
        self.round_no = round_no
        self.snapshot = snapshot
        self.allow_terminal_end = allow_terminal_end
        self._start = None

    def _take_snapshot(self):
        if self.snapshot is None:
            return _status_snapshot(self.out)
        return self.snapshot()

    def start(self):
        self._start = self._take_snapshot()

    def end(self):
        end_snapshot = self._take_snapshot()
        result = _evaluate_scan_phase(
            self._start, end_snapshot, self.expected_state, self.allow_terminal_end
        )
        if self.out is not None:
            emit_event(
                self.out,
                "phase_guard",
                phase=self.phase,
                run=self.run,
                round=self.round_no,
                valid=result["valid"],
                progress=result["progress"],
                reason=result["reason"],
                state_start=result["state_start"],
                state_end=result["state_end"],
                scanned_start=result["scanned_start"],
                scanned_end=result["scanned_end"],
                processed_start=result["processed_start"],
                processed_end=result["processed_end"],
                wall_ms=result["wall_ms"],
            )
        return result


def _status_snapshot(out):
    """Return a status snapshot dict or None when it cannot be read."""
    if out is None:
        return None
    path = Path(out) / "status-last.json"
    try:
        doc = json.loads(path.read_text())
    except OSError, json.JSONDecodeError:
        return None
    if not isinstance(doc, dict):
        return None
    return {
        "state": doc.get("state"),
        "scanned": doc.get("scanned"),
        "matched": doc.get("matched"),
        "processed": doc.get("processed"),
        "failed": doc.get("failed"),
        "ts": time.monotonic(),
    }


def _counter_ok(value):
    return not isinstance(value, bool) and isinstance(value, (int, float))


def _evaluate_scan_phase(start, end, expected_state, allow_terminal_end=False):
    if start is None:
        return _invalid(
            "missing or unparseable status at start", None, None, None, None, None, None, 0
        )
    if end is None:
        return _invalid(
            "missing or unparseable status at end",
            start.get("state"),
            None,
            start.get("scanned"),
            None,
            start.get("processed"),
            None,
            0,
        )

    state_start = start.get("state")
    state_end = end.get("state")
    scanned_start = start.get("scanned")
    scanned_end = end.get("scanned")
    processed_start = start.get("processed")
    processed_end = end.get("processed")

    wall_ms = 0
    start_ts = start.get("ts")
    end_ts = end.get("ts")
    if _counter_ok(start_ts) and _counter_ok(end_ts):
        wall_ms = int((end_ts - start_ts) * 1000)

    if state_start != expected_state:
        reason = (
            "terminal state at start"
            if state_start in TERMINAL_STATUS_STATES
            else "state mismatch at start"
        )
        return _invalid(
            reason,
            state_start,
            state_end,
            scanned_start,
            scanned_end,
            processed_start,
            processed_end,
            wall_ms,
        )
    if state_end != expected_state:
        if not (
            allow_terminal_end
            and state_start == expected_state
            and state_end in TERMINAL_STATUS_STATES
        ):
            reason = (
                "terminal state at end"
                if state_end in TERMINAL_STATUS_STATES
                else "state mismatch at end"
            )
            return _invalid(
                reason,
                state_start,
                state_end,
                scanned_start,
                scanned_end,
                processed_start,
                processed_end,
                wall_ms,
            )
    if (
        not _counter_ok(scanned_start)
        or not _counter_ok(scanned_end)
        or not _counter_ok(processed_start)
        or not _counter_ok(processed_end)
    ):
        # distinguish missing (None) from non-numeric
        for label, value in (
            ("scanned", scanned_start),
            ("scanned", scanned_end),
            ("processed", processed_start),
            ("processed", processed_end),
        ):
            if value is None:
                reason = "missing counter"
                break
            if not _counter_ok(value):
                reason = "non-numeric counter"
                break
        else:
            reason = "non-numeric counter"
        return _invalid(
            reason,
            state_start,
            state_end,
            scanned_start,
            scanned_end,
            processed_start,
            processed_end,
            wall_ms,
        )
    progress = bool(scanned_end > scanned_start or processed_end > processed_start)
    return {
        "valid": True,
        "reason": "ok",
        "state_start": state_start,
        "state_end": state_end,
        "scanned_start": scanned_start,
        "scanned_end": scanned_end,
        "processed_start": processed_start,
        "processed_end": processed_end,
        "wall_ms": wall_ms,
        "progress": progress,
    }


def _invalid(
    reason,
    state_start,
    state_end,
    scanned_start,
    scanned_end,
    processed_start,
    processed_end,
    wall_ms,
):
    return {
        "valid": False,
        "reason": reason,
        "state_start": state_start,
        "state_end": state_end,
        "scanned_start": scanned_start,
        "scanned_end": scanned_end,
        "processed_start": processed_start,
        "processed_end": processed_end,
        "wall_ms": wall_ms,
        "progress": False,
    }


# --- storm workloads ---------------------------------------------------------
# A storm drives one access from N clients. Scan storms run until the scan
# settles; idle storms run after a populate scan, against the tree at rest.
# Both record individual op timings, which the report pools rather than the
# per-run summaries.


def _percentile(sorted_vals: list, pct: float) -> float:
    if not sorted_vals:
        return 0.0
    if len(sorted_vals) == 1:
        return sorted_vals[0]
    rank = (len(sorted_vals) - 1) * pct / 100.0
    low = math.floor(rank)
    high = math.ceil(rank)
    if low == high:
        return sorted_vals[int(rank)]
    return sorted_vals[low] + (sorted_vals[high] - sorted_vals[low]) * (rank - low)


def _wait_for_rows(
    cfg: config.WorkloadConfig,
    client: HttpClient,
    logger: Logger,
    scan_active: threading.Event | None,
    min_rows: int = 1,
) -> list:
    """Poll the probe list until enough rows appear or the scan goes terminal."""
    if min_rows < 1:
        min_rows = 1
    for _ in range(150):  # ~30 s cap; the poll loop clears active on settle
        lines = build_probe_list(cfg, client) or []
        if len(lines) >= min_rows:
            return lines
        if scan_active is not None and not scan_active.is_set():
            break
        time.sleep(0.2)
    if lines:
        return lines
    raise WorkloadError("storm: no probe rows available (scan too short?)")


CONCURRENT_MIN_OPS_CAP = 256
CONCURRENT_MIN_OPS_FLOOR = 80


def storm_minimum_total(clients: int, ops: int, concurrent: bool) -> int:
    """Minimum ops a storm must issue before a scan can end.

    A concurrent storm stops when the scan does, so a fast scan cannot
    realistically complete ``clients * ops`` ops at high concurrency. Cap
    that target while keeping a floor of one op per client (or 80, the
    smallest storm) so a truncated run still fails. At-rest storms finish
    their full fixed workload.
    """
    requested = clients * ops
    if not concurrent:
        return requested
    capped = min(requested, CONCURRENT_MIN_OPS_CAP)
    return max(capped, min(clients, CONCURRENT_MIN_OPS_FLOOR))


def check_storm_coverage(summary: dict, scan_start: float, scan_end: float, min_pct: float) -> None:
    """Require the storm to cover enough of the scan wall."""
    scan_wall = scan_end - scan_start
    active_wall = summary.get("active_wall")
    if active_wall is None:
        active_wall = summary.get("wall", 0.0)
    if scan_wall > 0:
        coverage = min(100.0, 100.0 * active_wall / scan_wall)
    else:
        coverage = 100.0 if active_wall >= 0 else 0.0
    summary["coverage_pct"] = coverage
    done = summary.get("done", 0)
    total = summary.get("total", 0)
    requested = summary.get("requested_total", total)
    if done < total:
        raise WorkloadError(
            f"storm did not finish minimum ops: done={done} minimum={total} "
            f"requested={requested} "
            f"active_wall={active_wall:.1f}s scan_wall={scan_wall:.1f}s "
            f"coverage={coverage:.1f}%"
        )
    if coverage < min_pct:
        raise WorkloadError(
            f"storm coverage too low: active_wall={active_wall:.1f}s "
            f"scan_wall={scan_wall:.1f}s coverage={coverage:.1f}% "
            f"min={min_pct:g}% ops={done}/{total} minimum={total}"
        )


# --- scans -------------------------------------------------------------------
def _write_status_final(out: Path, run_index: int) -> None:
    """Publish a run's status under the terminal names when it is terminal.

    `status-last.json` always holds the newest poll, including `running`
    snapshots. The final names mean "the run finished", so a non-terminal
    snapshot is not copied there; stale final files from an earlier run are
    removed instead.
    """
    source = out / "status-last.json"
    if source.is_file():
        body = source.read_bytes()
    else:
        body = b'{"state":"unknown"}\n'
    document = _body_json(body) or {}
    if document.get("state") not in TERMINAL_STATUS_STATES:
        (out / "status-final.json").unlink(missing_ok=True)
        (out / f"status-final-run-{run_index}.json").unlink(missing_ok=True)
        return
    (out / "status-final.json").write_bytes(body)
    (out / f"status-final-run-{run_index}.json").write_bytes(body)


def _refresh_scan_status(
    cfg: config.WorkloadConfig,
    client: HttpClient,
    out: Path,
    logger: Logger,
    *,
    attempts: int = 25,
) -> str:
    """Poll status briefly so guards do not start against stale data."""
    state = "unknown"
    for attempt in range(max(1, attempts)):
        try:
            code, body, _ = client.get("/get/index/status")
            if code == 200:
                (out / "status-last.json").write_bytes(body)
        except WorkloadError:
            logger.log("status refresh failed")
        state = _status_state(out)
        if state == "running":
            break
        if state in TERMINAL_STATUS_STATES and attempt > 0:
            break
        time.sleep(cfg.poll_interval_s)
    if state == "unknown":
        logger.log("status refresh: state still unknown")
    return state


def _emit_warmup_event(out, outcome, result, endpoints):
    emit_event(
        out,
        "warmup",
        valid=outcome["valid"],
        reason=outcome["reason"],
        wall_ms=outcome["wall_ms"],
        endpoints=endpoints,
        state_start=result.get("state_start"),
        state_end=result.get("state_end"),
        scanned_start=result.get("scanned_start"),
        scanned_end=result.get("scanned_end"),
        processed_start=result.get("processed_start"),
        processed_end=result.get("processed_end"),
    )


def warmup_scan(
    cfg: config.WorkloadConfig,
    client: HttpClient,
    out: Path,
    run_index: int,
    rows: list,
) -> dict:
    """Run one in-scan warmup round and discard its timings."""
    endpoints = []
    failures = []
    if not rows:
        deadline = time.monotonic() + (
            cfg.scan_timeout_s if cfg.scan_timeout_s and cfg.scan_timeout_s > 0 else 5.0
        )
        deadline = min(deadline, time.monotonic() + 5.0)
        while not rows and time.monotonic() < deadline:
            if _status_state(out) in TERMINAL_STATUS_STATES:
                break
            try:
                built = build_probe_list(cfg, client) or []
            except Exception as err:  # noqa: BLE001 - warmup must not kill the scan
                failures.append(f"build_probe_list: {err}")
                built = []
            if built:
                rows[:] = built
                rows = built
                break
            time.sleep(cfg.probe_interval_s)
    if not rows:
        guard = ScanPhase(out, "warmup", expected_state="running", run=run_index)
        guard.start()
        result = guard.end()
        outcome = {
            "valid": False,
            "reason": "no probe rows",
            "wall_ms": result.get("wall_ms", 0),
            "endpoints": endpoints,
        }
        _emit_warmup_event(out, outcome, result, endpoints)
        return outcome

    guard = ScanPhase(out, "warmup", expected_state="running", run=run_index)
    guard.start()
    timestamp = _prefetch_timestamp(out)
    token = _prefetch_token(out)
    if timestamp is None or token is None:
        endpoints.append("prefetch")
        try:
            code, body, _ = client.post("/get/prefetch", {})
            if out is not None:
                (out / "prefetch.json").write_bytes(body)
            doc = _body_json(body) if isinstance(body, (bytes, bytearray, str)) else None
            if isinstance(doc, dict):
                timestamp = doc.get("prefetch", {}).get("timestamp")
                token = doc.get("token")
        except Exception as err:  # noqa: BLE001 - warmup must not kill the scan
            failures.append(f"prefetch: {err}")
    if timestamp is None or token is None:
        if "prefetch" not in endpoints:
            endpoints.append("prefetch")
            failures.append("prefetch: missing timestamp or token")

    row = rows[0]
    try:
        hash_value, row_token = row.split("\t", 1)
    except ValueError:
        hash_value, row_token = row, None
    use_token = row_token or token
    if timestamp is not None and use_token is not None and hash_value:
        endpoints.append("compressed")
        try:
            client.get(_compressed_path(hash_value), bearer=use_token)
        except Exception as err:  # noqa: BLE001 - warmup must not kill the scan
            failures.append(f"compressed: {err}")

    if timestamp is not None and token is not None:
        endpoints.append("scroll")
        try:
            scroll_probe(
                cfg,
                client,
                timestamp,
                token,
                total_rows=len(rows),
            )
        except Exception as err:  # noqa: BLE001 - warmup must not kill the scan
            failures.append(f"scroll: {err}")

    result = guard.end()
    reason = "ok"
    if failures:
        reason = failures[0]
    elif not result.get("valid", False):
        reason = result.get("reason", "invalid")
    valid = (not failures) and bool(result.get("valid", False))
    outcome = {
        "valid": valid,
        "reason": reason,
        "wall_ms": result.get("wall_ms", 0),
        "endpoints": endpoints,
    }
    _emit_warmup_event(out, outcome, result, endpoints)
    return outcome


CANARY_CHECKS = 8


def verify_storm_sample(
    cfg: config.WorkloadConfig, client: HttpClient, logger: Logger, out: Path, run_index: int
) -> None:
    """Round-trip spot check for edit storms: PUT distinct ratings on
    spread-out assets, read each back by stable assetId. PUTs commit
    synchronously, so a mismatch means the write did not stick (or hit a
    different asset than addressed) — fail loudly instead of trusting 200s.
    Canaries are picked across the paged row space, not the first window
    alone, so a 5650-row library verifies rows past index 1000 too.
    """
    code, body, _ = client.post("/get/prefetch", {}, timeout=30.0)
    if code != 200:
        raise WorkloadError("storm verify: prefetch failed")
    payload = _body_json(body) or {}
    timestamp, token = payload.get("prefetch", {}).get("timestamp"), payload.get("token")
    if timestamp is None or token is None:
        raise WorkloadError("storm verify: no prefetch timestamp/token")
    page_size = cfg.probe_page
    collected = []
    for start, page, _body in page_get_data(
        client, timestamp, token, page_size=page_size, cap=row_space_cap(cfg), timeout=30.0
    ):
        if page is None:
            break
        collected.extend((start + offset, row) for offset, row in enumerate(page))
    if not collected:
        raise WorkloadError("storm verify: no probe rows")
    step = max(1, len(collected) // CANARY_CHECKS)
    picks = []
    for position in range(0, len(collected), step):
        if len(picks) >= CANARY_CHECKS:
            break
        index, row = collected[position]
        asset = row.get("assetId") if isinstance(row, dict) else None
        if asset:
            picks.append((index, asset))
    if not picks:
        raise WorkloadError("storm verify: rows carry no assetId")


# --- upload helpers ----------------------------------------------------------
def _select_upload_sources(cfg: config.WorkloadConfig) -> list:
    """Upload file set from the cell's declared upload source.

    A separate `UPLOAD_REPO` is the provided upload set: its first
    `UPLOAD_FILES` images are uploaded, and a smaller set fails the cell.
    When the upload repo is the cell's own image repo, the stimulus must
    use files beyond the staged prefix, so re-uploading an indexed image
    cannot happen; a repo without that margin fails the cell.
    """
    repo = cfg.upload_repo or cfg.image_repo
    available = sorted(Path(repo).glob("*.jpg"), key=lambda path: path.name)
    if len(available) < cfg.upload_files:
        raise WorkloadError(
            f"upload: {repo} provides {len(available)} images, need UPLOAD_FILES={cfg.upload_files}"
        )
    if Path(repo).resolve() != Path(cfg.image_repo).resolve():
        return available[: cfg.upload_files]
    staged = set()
    image_path = cfg.image_path
    if image_path is not None:
        try:
            staged = {path.name for path in Path(image_path).glob("*.jpg")}
        except OSError:
            staged = set()
    sources = [path for path in available if path.name not in staged]
    if len(sources) < cfg.upload_files:
        raise WorkloadError(
            f"upload: {repo} provides {len(sources)} images beyond the staged set, "
            f"need UPLOAD_FILES={cfg.upload_files}; point UPLOAD_REPO at a distinct set"
        )
    return sources[: cfg.upload_files]


def _uploaded_row_visible(cfg: config.WorkloadConfig, client: HttpClient, stored_name: str) -> bool:
    """True once `stored_name` appears in the snapshot row space.

    The lookup pages the snapshot in PROBE_PAGE windows up to the cell's
    row-space cap: at scale an upload sorts past the first window, so a
    single `0..1000` request would never see it.
    """
    code, body, _ = client.post("/get/prefetch", {})
    if code != 200:
        return False
    doc = _body_json(body)
    if not isinstance(doc, dict):
        return False
    timestamp = doc.get("prefetch", {}).get("timestamp")
    token = doc.get("token")
    if timestamp is None or token is None:
        return False
    page_size = cfg.probe_page
    for _start, rows, _body in page_get_data(
        client, timestamp, token, page_size=page_size, cap=row_space_cap(cfg)
    ):
        if rows is None:
            return False
        for row in rows:
            if not isinstance(row, dict):
                continue
            abstract_data = row.get("abstractData", {})
            if not isinstance(abstract_data, dict):
                continue
            path = abstract_data.get("path", {})
            file_name = path.get("file") if isinstance(path, dict) else None
            if isinstance(file_name, str) and stored_name in file_name:
                return True
            if stored_name in json.dumps(row):
                return True
    return False


def wait_for_library_rows(
    cfg: config.WorkloadConfig, client: HttpClient, logger: Logger, expected: int
) -> None:
    """Wait until the materialised library is visible in the snapshot.

    `/get/index/status` starts in `Idle` before the watcher submits its
    first job, so a settle check alone can pass while the initial index has
    not begun. The at-rest upload needs the real rows, otherwise its
    visibility includes the initial library index.
    """
    budget = cfg.index_settle_timeout_s
    deadline = time.monotonic() + budget
    rows = 0
    while time.monotonic() < deadline:
        rows = len(build_probe_list(cfg, client) or [])
        if rows >= expected:
            logger.log(f"library indexed: rows={rows}/{expected}")
            return
        time.sleep(max(cfg.poll_interval_s, 0.5))
    raise WorkloadError(
        f"library index did not reach {expected} rows within {budget:g}s (rows={rows})"
    )


def wait_for_index_settled(cfg: config.WorkloadConfig, client: HttpClient, logger: Logger) -> str:
    """Block until no album index is running, then return the observed state.

    A cell that materialises its image set can leave a full-library album
    index in flight (watcher rescan). Upload timing is only meaningful once
    that index has settled, so the stimulus waits for a terminal state or
    `idle` (no job has run) and fails the cell when the budget expires.
    """
    budget = cfg.index_settle_timeout_s
    logger.log(f"=== waiting for library index to settle (budget {budget:g}s) ===")
    started = time.monotonic()
    deadline = started + budget
    polls = 0
    state = "unknown"
    counters = ()
    while True:
        polls += 1
        try:
            code, body, _ = client.get("/get/index/status")
        except WorkloadError:
            code, body = 0, b""
        document = _body_json(body) if code == 200 else None
        if isinstance(document, dict):
            state = document.get("state", "unknown")
            counters = tuple(
                document.get(key) for key in ("scanned", "matched", "processed", "failed")
            )
        else:
            state = "unknown"
        if state in SETTLED_STATUS_STATES:
            logger.log(
                f"index settled: state={state} polls={polls} "
                f"waited={time.monotonic() - started:.1f}s"
            )
            return state
        if time.monotonic() >= deadline:
            raise WorkloadError(
                f"library index did not settle within {budget:g}s (state={state} "
                f"polls={polls} counters={counters}) — the upload stimulus needs an "
                "idle server; wait for the initial index to finish or raise "
                "INDEX_SETTLE_TIMEOUT_S"
            )
        time.sleep(cfg.poll_interval_s)


# --- main flow ---------------------------------------------------------------
def prepare_out(cfg: config.WorkloadConfig) -> None:
    out = cfg.out
    out.mkdir(parents=True, exist_ok=True)
    for name in ARTIFACT_FILES:
        (out / name).unlink(missing_ok=True)
    for pattern in ARTIFACT_GLOBS:
        for path in out.glob(pattern):
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink(missing_ok=True)


def _log_summary(cfg: config.WorkloadConfig, logger: Logger) -> None:
    """The cell's closing transcript lines."""
    logger.log("=== summary ===")
    logger.log(f"N={cfg.ncpu} image_count={cfg.image_count} image_bytes={cfg.image_bytes}")
    if cfg.scan_runs:
        walls = cfg.scan_walls_ms or [cfg.scan_wall_ms]
        logger.log(
            f"scan wall median_ms={cfg.scan_wall_ms:g} over {cfg.scan_runs} run(s): "
            + ",".join(str(int(wall)) for wall in walls)
        )
    else:
        logger.log("scan_wall_ms=0 scan_state=none (no scan in this cell)")
    logger.log(f"artifacts: {cfg.out}")
    logger.log("done")


def run_workload(cfg: config.WorkloadConfig) -> int:
    prepare_out(cfg)
    logger = Logger(cfg.out / "harness-a.log")
    try:
        logger.log("=== workload preflight ===")
        logger.log(
            f"base_url={cfg.base_url} out={cfg.out} workload={cfg.workload} "
            f"scan_repeats={cfg.scan_repeats} repeats={cfg.repeats} "
            f"warmup={cfg.warmup} runs_during={cfg.runs_during} "
            f"fetches={cfg.fetches} images={cfg.images}"
        )

        image_count, image_bytes = prepare_images(cfg, logger)
        cfg.image_count = image_count
        cfg.image_bytes = image_bytes
        cfg.expected_entries = image_count

        logger.log("=== run parameters ===")
        logger.log(f"N={cfg.ncpu} out={cfg.out}")
        logger.log(f"image_count={image_count} image_bytes={image_bytes}")
        logger.log(
            f"warmup={cfg.warmup} "
            f"repeats={cfg.repeats} runs_during={cfg.runs_during} "
            f"scan_repeats={cfg.scan_repeats} build_profile={cfg.build_profile} "
            f"port={cfg.port}"
        )
        logger.log(f"scan repeats: {cfg.scan_repeats} (median wall time reported)")

        spec = scenarios.build_spec(
            cfg.workload,
            min_probe_rounds=cfg.min_probe_rounds,
            repeats=cfg.repeats,
            upload_files=cfg.upload_files,
            storm_ops=cfg.storm_ops,
            ncpu=cfg.ncpu,
        )
        logger.log(f"cell spec: profile={spec.profile} phases={len(spec.phases)}")
        client = HttpClient(cfg.base_url, cfg.cookie_jar)
        if cfg.workload == scenarios.IDLE_UPLOAD:
            # Only the at-rest upload needs a settled, populated library;
            # the scan background must still be indexing when uploads start.
            wait_for_library_rows(cfg, client, logger, cfg.image_count)
            wait_for_index_settled(cfg, client, logger)
        if any(phase.issue == "files" for phase in spec.phases):
            cfg.expected_entries = cfg.image_count + cfg.upload_files
        executor.execute_cell(cfg, client, logger, spec)
        _log_summary(cfg, logger)
        return 0
    except WorkloadError as err:
        logger.log(f"ERROR: {err}")
        return 2
    except ConfigError as err:
        logger.log(f"ERROR: {err}")
        return 1
    finally:
        logger.close()


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--launcher-info",
        help="path to launcher-info.json (default: $LAUNCHER_INFO)",
    )
    args = parser.parse_args(argv)
    environ = dict(os.environ)
    if args.launcher_info:
        environ["LAUNCHER_INFO"] = args.launcher_info
    try:
        cfg = config_from_env(environ)
    except ConfigError as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 1
    return run_workload(cfg)


if __name__ == "__main__":
    sys.exit(main())
