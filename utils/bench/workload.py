#!/usr/bin/env python3
"""Workload half of a benchmark cell, against a launcher-owned server.

Consumes `launcher-info.json` written by `utils/bench/backend_launcher.py`
and implements the measurement protocol (line formats compatible with
the retired shell driver, so historical cells keep parsing):
image-set materialisation, repeated album-index scans, during-scan random
compressed fetches plus hyperfine prefetch/edit-rating probes, a post-scan
sequential sweep, post-scan hyperfine baselines, and write-probe verification.

Artifacts are written in the bench transcript format so `utils/bench/extract_bench.py`
parses either producer unchanged:

  harness-a.log                    run transcript (harness line formats)
  status-last.json                 most recent /get/index/status body
  status-final.json                last run's terminal status
  status-final-run-<n>.json        per-run terminal status
  scan-walls.tsv                   run index + wall ms
  prefetch.json / get-data.json    raw probe-request records (debug)
  probe-seq.tsv                    hash<TAB>code<TAB>seconds sweep
  rounds-run-<n>/                  per-run probe rounds
    hyperfine-round-<n>.json
    hyperfine-round-<n>.out
    probe-round-<n>.tsv
  hyperfine.json                   post-scan hyperfine export
  verify-row.json / verify-metadata.json
  probe-phase.log                  during-scan transcript
  probe-phase.log                  during-scan transcript
  events.jsonl                    structured JSONL event stream (schema v=1):
                                    one JSON object per line, each carrying
                                    {"v": 1, "type": ..., "t": <ISO wall clock>}.
                                    v1 types and required keys:
                                      probe_round: round, rows, state,
                                        fetch_n, fetch_mean_ms (plus fetch min/max),
                                        prefetch/edit hyperfine means
                                      storm_sample: profile, client, op, latency_ms, code
                                      storm_summary: profile, kind, done, total, median,
                                        p95, max, failed, wall
                                      upload_file: file, submit_ms, visible_ms
                                      upload_summary: submit_p50, visible_p50
                                      scan_state: run, state
                                      verify_result: kind, matched, total
                                    Incompatible schema changes bump v; readers switch on v.


The module is importable and stdlib-only. Direct HTTP uses urllib with the
launcher's cookie jar; hyperfine still receives the same curl command strings
as the shell harness (tests replace the hyperfine binary).

Environment contract (on top of the launcher's):

  LAUNCHER_INFO   path to launcher-info.json (required)
  OUT             artifact directory (default: launcher-info.out)
  NCPU            cores for the CPU column (default 4)
  IMAGES          images per cell, copied from IMAGE_REPO (default 100)
  IMAGE_REPO      pre-seeded image directory (required, read-only);
                  the first IMAGES entries in sorted order are copied
  IMAGE_PATH      cell image directory (default OUT/images)
  REPEATS         post-scan hyperfine runs (default 3)
  WARMUP          post-scan hyperfine warmup runs (default 1)
  RUNS_DURING     per-round hyperfine runs during a scan (default 3; the
                  per-round warmup is always 0)
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
  WORKLOAD        control (default) | storm-<access>-<level> |
                  storm-idle-<access>-<level> | upload, where access is
                  read, thumb or edit and level is 8 or 16n (16*NCPU):
                  control runs probe rounds during the scan; storm profiles
                  run sustained concurrent prefetch, thumbnail or
                  edit-rating load until the scan terminal state;
                  storm-idle profiles run the populate scans, then one
                  burst at rest; upload measures the write path end to end
                  and runs the control probe rounds during the next scan
  UPLOAD_FILES    upload stimulus size (default 20); the image repo must
                  provide at least this many images
  UPLOAD_TIMEOUT_S visibility poll budget per uploaded file (default 30);
                  a file that never becomes visible fails the cell
  UPLOAD_POST_TIMEOUT_S  budget for one upload POST (default 60)
  INDEX_SETTLE_TIMEOUT_S  upload profile: budget for the library index to
                  settle before the upload stimulus, so the write path is
                  measured on an idle server (default 600)
  STORM_CLIENTS   concurrent storm clients; named levels derive 8 or 16*NCPU
  STORM_OPS       minimum ops per storm client (default 10)
  STORM_MIN_COVERAGE_PCT  concurrent storm wall as a percentage of scan wall
                  (default 50); below it the cell fails
  HYPERFINE_BIN   hyperfine binary (required; managed via `just install hyperfine`)

Exit codes: 0 completed; 1 config/preflight; 2 runtime failure (including a
scan that did not reach `completed` and a failed write-probe verification).
"""

from __future__ import annotations

import argparse
import http.cookiejar
import itertools
import json
import math
import os
import random
import shutil
import statistics
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from concurrent import futures
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent

# The preflight checklist owns the shared environment guards (hyperfine);
# config.py owns the typed config and its defaults.
sys.path.insert(0, str(SCRIPT_DIR))
import config  # noqa: E402
import preflight  # noqa: E402
from config import ConfigError  # noqa: E402

TERMINAL_STATUS_STATES = ("completed", "failed", "canceled")
# `idle` means no album-index job has run, so there is nothing to wait for.
SETTLED_STATUS_STATES = (*TERMINAL_STATUS_STATES, "idle")

ARTIFACT_FILES = (
    "harness-a.log",
    "events.jsonl",
    "status-last.json",
    "status-final.json",
    "scan-walls.tsv",
    "scan-done.marker",
    "prefetch.json",
    "get-data.json",
    "probe-seq.tsv",
    "probe-phase.log",
    "hyperfine.json",
    "hyperfine-fallback.txt",
    "index-trigger.json",
    "verify-row.json",
    "verify-metadata.json",
)
ARTIFACT_GLOBS = ("rounds-run-*", "status-final-run-*.json")


class WorkloadError(Exception):
    """Runtime failure after the cell started."""


# --- configuration -----------------------------------------------------------
HYPERFINE_INSTALL_HINT = f"hyperfine is required ({preflight.HYPERFINE_HINT})"


def resolve_hyperfine(environ: dict) -> str:
    """Resolve hyperfine through the shared preflight check."""
    check = preflight.check_hyperfine(environ)
    if not check.ok:
        raise ConfigError(check.message())
    return check.detail


def config_from_env(environ: dict) -> config.WorkloadConfig:
    """Build workload config once; consumers read attributes."""
    return config.WorkloadConfig.from_env(environ, hyperfine_lookup=resolve_hyperfine)


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


# --- hyperfine ---------------------------------------------------------------
def cmd_prefetch(cfg: config.WorkloadConfig) -> str:
    return (
        f"curl -s -o /dev/null -b {cfg.cookie_jar} -X POST "
        f"-H 'Content-Type: application/json' -d '{{}}' "
        f"-w '%{{http_code}} %{{time_total}}\\n' {cfg.base_url}/get/prefetch"
    )


def cmd_compressed(cfg: config.WorkloadConfig, hash_value: str, token: str) -> str:
    return (
        f"curl -s -o /dev/null -b {cfg.cookie_jar} "
        f"-H 'Authorization: Bearer {token}' "
        f"-w '%{{http_code}} %{{time_total}}\\n' "
        f"{cfg.base_url}{_compressed_path(hash_value)}"
    )


def cmd_edit_rating(cfg: config.WorkloadConfig, timestamp) -> str:
    return (
        f"curl -s -o /dev/null -b {cfg.cookie_jar} -X PUT "
        f"-H 'Content-Type: application/json' "
        f'-d \'{{"indexArray":[0],"timestamp":{timestamp},"rating":3}}\' '
        f"-w '%{{http_code}} %{{time_total}}\\n' {cfg.base_url}/put/edit_rating"
    )


def _hyperfine_results(path: Path) -> list:
    try:
        doc = json.loads(path.read_text())
    except OSError, json.JSONDecodeError:
        return []
    results = doc.get("results") if isinstance(doc, dict) else None
    return results if isinstance(results, list) else []


def _result_name(result: dict) -> str:
    return str(result.get("name") or result.get("command") or "")


def _wall_clock(result: dict) -> dict:
    # hyperfine >=1.19 exports timings flat on each result; older versions
    # nested them under summary.time_wall_clock. Accept both.
    if isinstance(result, dict) and ("mean" in result or "median" in result):
        times = result.get("times")
        count = len(times) if isinstance(times, list) else result.get("count")
        if count is None:
            exit_codes = result.get("exit_codes")
            if isinstance(exit_codes, list):
                count = len(exit_codes)
        return {
            "mean": result.get("mean"),
            "median": result.get("median"),
            "stddev": result.get("stddev"),
            "count": count,
        }
    if isinstance(result, dict):
        nested = result.get("summary", {})
        if isinstance(nested, dict):
            nested = nested.get("time_wall_clock", {})
        if isinstance(nested, dict):
            return nested
    return {}


def run_hyperfine(
    cfg: config.WorkloadConfig,
    warmup: int,
    runs: int,
    export_json: Path,
    benches: list,
    logger: Logger,
    output_path: Path | None = None,
) -> list:
    binary = cfg.hyperfine
    if not binary:
        raise WorkloadError(HYPERFINE_INSTALL_HINT)
    command = [binary, "--warmup", str(warmup), "-r", str(runs), "--export-json", str(export_json)]
    for name, shell in benches:
        command.extend(["-n", name, shell])
    export_json.unlink(missing_ok=True)
    result = subprocess.run(command, capture_output=True, text=True)
    combined = result.stdout + result.stderr
    if output_path is not None:
        output_path.write_text(combined)
    if result.returncode != 0:
        raise WorkloadError(f"hyperfine exited {result.returncode}")
    names = ", ".join(name for name, _ in benches)
    logger.log(f"hyperfine: {names} runs={runs} ok")
    return _hyperfine_results(export_json)


def _summary_line(results: list) -> str:
    parts = []
    for result in results:
        wall = _wall_clock(result)
        parts.append(
            f"{_result_name(result)} mean={wall.get('mean')} s "
            f"median={wall.get('median')} s runs={wall.get('count')}"
        )
    return " | ".join(parts)


# --- during-scan probes ------------------------------------------------------
def random_fetch_round(
    cfg: config.WorkloadConfig,
    client: HttpClient,
    transcript: ProbeTranscript,
    round_no: int,
    rows: int,
    state: str,
    lines: list,
    round_dir: Path,
) -> None:
    sample = round_dir / f"probe-round-{round_no}.tsv"
    picks = []
    if lines and cfg.fetches:
        picks = random.sample(lines, min(cfg.fetches, len(lines)))
    records = []
    for line in picks:
        hash_value, token = line.split("\t", 1)
        try:
            code, _, elapsed = client.get(_compressed_path(hash_value), bearer=token)
        except WorkloadError:
            code, elapsed = 0, 0.0
        records.append(f"{hash_value}\t{code}\t{elapsed!r}")
    sample.write_text("\n".join(records) + ("\n" if records else ""))
    times = [float(record.split("\t")[2]) for record in records]
    fetch_mean_s = (sum(times) / len(times)) if times else 0.0
    fetch_min_s = min(times) if times else 0.0
    fetch_max_s = max(times) if times else 0.0
    emit_event(
        cfg.out,
        "probe_round",
        round=round_no,
        rows=rows,
        state=state,
        fetch_n=len(times),
        fetch_mean=fetch_mean_s,
        fetch_mean_s=fetch_mean_s,
        fetch_mean_ms=fetch_mean_s * 1000.0,
        fetch_min=fetch_min_s,
        fetch_min_s=fetch_min_s,
        fetch_min_ms=fetch_min_s * 1000.0,
        fetch_max=fetch_max_s,
        fetch_max_s=fetch_max_s,
        fetch_max_ms=fetch_max_s * 1000.0,
        fetch_ms={
            "n": len(times),
            "mean": fetch_mean_s * 1000.0,
            "min": fetch_min_s * 1000.0,
            "max": fetch_max_s * 1000.0,
        },
    )


def scroll_probe(
    cfg: config.WorkloadConfig,
    client: HttpClient,
    timestamp,
    token,
    *,
    total_rows=None,
    positions: int | None = 3,
    timeout: float = 30.0,
    round_dir: Path | None = None,
    round_no: int | None = None,
    out: Path | None = None,
    phase: str = "during",
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

    # Persist a per-round artifact when a round dir is provided.
    if round_dir is not None and round_no is not None:
        try:
            round_dir = Path(round_dir)
            round_dir.mkdir(parents=True, exist_ok=True)
            path = round_dir / f"scroll-round-{round_no}.tsv"
            lines_out = []
            for value in scrollbar:
                lines_out.append(f"scrollbar\t200\t{value!r}")
            for value in rows:
                lines_out.append(f"rows\t200\t{value!r}")
            for value in data:
                lines_out.append(f"data\t200\t{value!r}")
            path.write_text("\n".join(lines_out) + ("\n" if lines_out else ""))
        except OSError:
            pass

    # Persist a cell-level artifact when an out dir is provided.
    if out is not None:
        try:
            out = Path(out)
            out.mkdir(parents=True, exist_ok=True)
            path = out / "scroll-probe.tsv"
            lines_out = []
            for value in scrollbar:
                lines_out.append(f"scrollbar\t200\t{value!r}")
            for value in rows:
                lines_out.append(f"rows\t200\t{value!r}")
            for value in data:
                lines_out.append(f"data\t200\t{value!r}")
            path.write_text("\n".join(lines_out) + ("\n" if lines_out else ""))
        except OSError:
            pass

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
    round_dir: Path,
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
        )
        return False

    random_fetch_round(cfg, client, transcript, round_no, rows, state, lines, round_dir)

    # Scroll probe: exercise the scroll-bar / row / data read paths that the
    # frontend drives while scrolling, mirroring useFetchRows/useFetchImgs.
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
                round_dir=round_dir,
                round_no=round_no,
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

    json_path = round_dir / f"hyperfine-round-{round_no}.json"
    benches = [
        ("prefetch", cmd_prefetch(cfg)),
        ("edit-rating", cmd_edit_rating(cfg, timestamp)),
    ]
    try:
        results = run_hyperfine(
            cfg,
            warmup=0,
            runs=cfg.runs_during,
            export_json=json_path,
            benches=benches,
            logger=logger,
            output_path=round_dir / f"hyperfine-round-{round_no}.out",
        )
    except WorkloadError:
        transcript.log(
            f"probe round={round_no} rows={rows} state={state} hyperfine FAILED "
            f"(see {round_dir / f'hyperfine-round-{round_no}.out'})"
        )
        emit_event(
            cfg.out,
            "probe_round",
            round=round_no,
            rows=rows,
            state=state,
            hyperfine_failed=True,
            reason="hyperfine error",
        )
        return False
    if len(results) != 2:
        transcript.log(
            f"probe round={round_no} rows={rows} state={state} hyperfine json has no results"
        )
        emit_event(
            cfg.out,
            "probe_round",
            round=round_no,
            rows=rows,
            state=state,
            hyperfine_failed=True,
            reason="no results",
        )
        return False
    summary = _summary_line(results)
    transcript.log(f"probe round={round_no} rows={rows} state={state}")
    prefetch_wall = _wall_clock(results[0])
    edit_wall = _wall_clock(results[1])
    emit_event(
        cfg.out,
        "probe_round",
        round=round_no,
        rows=rows,
        state=state,
        hyperfine_summary=summary,
        prefetch_mean=prefetch_wall.get("mean"),
        prefetch_median=prefetch_wall.get("median"),
        prefetch_runs=prefetch_wall.get("count"),
        prefetch_mean_ms=(prefetch_wall.get("mean") or 0) * 1000.0,
        prefetch_median_ms=(prefetch_wall.get("median") or 0) * 1000.0,
        edit_mean=edit_wall.get("mean"),
        edit_median=edit_wall.get("median"),
        edit_runs=edit_wall.get("count"),
        edit_mean_ms=(edit_wall.get("mean") or 0) * 1000.0,
        edit_median_ms=(edit_wall.get("median") or 0) * 1000.0,
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


def _run_index_from_round_dir(round_dir):
    try:
        name = Path(round_dir).name
        if name.startswith("rounds-run-"):
            return int(name.split("rounds-run-", 1)[1])
    except ValueError, AttributeError:
        pass
    return 1


def probe_phase(
    cfg: config.WorkloadConfig,
    client: HttpClient,
    logger: Logger,
    round_dir: Path,
    cached_rows: list | None = None,
) -> dict:
    out = cfg.out
    row_cache = cached_rows if isinstance(cached_rows, list) else None
    transcript = ProbeTranscript(logger, out / "probe-phase.log")
    marker = out / "scan-done.marker"
    waited = 0.0
    rounds_total = 0
    rounds_valid = 0
    invalid_reasons = []
    state = "unknown"
    lines = []
    last_build = None
    phase_start_snapshot = _status_snapshot(out)
    max_scanned = (
        phase_start_snapshot.get("scanned") if isinstance(phase_start_snapshot, dict) else None
    )
    max_processed = (
        phase_start_snapshot.get("processed") if isinstance(phase_start_snapshot, dict) else None
    )
    if not _counter_ok(max_scanned):
        max_scanned = None
    if not _counter_ok(max_processed):
        max_processed = None
    run_index = _run_index_from_round_dir(round_dir)
    try:
        while True:
            if marker.is_file():
                break
            state = _status_state(out)
            if state in TERMINAL_STATUS_STATES:
                break

            now = time.monotonic()
            if (
                not lines
                or last_build is None
                or (cfg.probe_refresh_s > 0 and (now - last_build) >= cfg.probe_refresh_s)
            ):
                try:
                    built = build_probe_list(cfg, client) or []
                except Exception as err:  # noqa: BLE001 - keep the outcome reachable
                    transcript.log(f"probe phase: build_probe_list failed: {err}")
                    built = []
                if built:
                    lines = built
                    if row_cache is not None:
                        row_cache[:] = list(lines)
                elif row_cache:
                    lines = list(row_cache)
                else:
                    lines = []
                last_build = now

            rows = len(lines)
            state = _status_state(out)

            if marker.is_file() or state in TERMINAL_STATUS_STATES:
                break

            if rows > 0:
                rounds_total += 1
                round_no = rounds_total
                guard = ScanPhase(
                    out,
                    "probe",
                    expected_state="running",
                    run=run_index,
                    round_no=round_no,
                    snapshot=lambda: _status_snapshot(out),
                )
                guard.start()
                probe_ok = False
                probe_error = None
                try:
                    probe_ok = bool(
                        probe_round(
                            cfg,
                            client,
                            logger,
                            transcript,
                            round_no,
                            rows,
                            state,
                            lines,
                            round_dir,
                        )
                    )
                except Exception as err:  # noqa: BLE001 - a probe must not kill the scan
                    probe_error = err
                    transcript.log(f"probe round {round_no} failed; continuing: {err}")
                result = guard.end()
                snap = _status_snapshot(out)
                if isinstance(snap, dict):
                    sc = snap.get("scanned")
                    pr = snap.get("processed")
                    if _counter_ok(sc):
                        if max_scanned is None or sc > max_scanned:
                            max_scanned = sc
                    if _counter_ok(pr):
                        if max_processed is None or pr > max_processed:
                            max_processed = pr
                if probe_ok and result.get("valid"):
                    rounds_valid += 1
                else:
                    if probe_error is not None:
                        reason = f"probe round failed: {probe_error}"
                    elif not probe_ok:
                        reason = "probe round failed"
                    else:
                        reason = result.get("reason", "invalid")
                    invalid_reasons.append(reason)
                    transcript.log(
                        f"warn: probe round={round_no} rows={rows} state={state} invalid: {reason}"
                    )

            if marker.is_file():
                break
            state = _status_state(out)
            if state in TERMINAL_STATUS_STATES:
                break
            if cfg.scan_timeout_s > 0 and waited >= cfg.scan_timeout_s:
                transcript.log(
                    f"probe phase: rows={len(lines)} state={state} waited~{waited:g}s "
                    "(explicit SCAN_TIMEOUT_S), probing what exists"
                )
                break
            time.sleep(cfg.probe_interval_s)
            waited += cfg.probe_interval_s
    except (WorkloadError, OSError) as err:
        reason = f"probe phase failed: {err}"
        invalid_reasons.append(reason)
        transcript.log(reason)
    finally:
        if rounds_total == 0:
            transcript.log(f"probe phase: no image rows before the scan settled (state={state})")
        else:
            transcript.log(
                f"probe phase: rounds={rounds_total} valid={rounds_valid} "
                f"rows={len(lines)} state_at_break={_status_state(out)}"
            )
        transcript.close()

    progress = False
    if isinstance(phase_start_snapshot, dict):
        ss = phase_start_snapshot.get("scanned")
        sp = phase_start_snapshot.get("processed")
        if _counter_ok(ss) and _counter_ok(max_scanned) and max_scanned > ss:
            progress = True
        if _counter_ok(sp) and _counter_ok(max_processed) and max_processed > sp:
            progress = True

    return {
        "rounds_total": rounds_total,
        "rounds_valid": rounds_valid,
        "invalid_reasons": invalid_reasons,
        "progress": progress,
    }


# --- storm workloads ---------------------------------------------------------
# storm-<access>-<level>: concurrent load on one access while a scan runs.
# storm-idle-<access>-<level>: one populate scan, then the same load
# against the tree at rest (no concurrent scan).
# Both record one summary per run and individual op timings; the reported
# statistics pool the op timings, never the run summaries.


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


def run_storm(
    cfg: config.WorkloadConfig,
    client: HttpClient,
    logger: Logger,
    round_dir: Path,
    run_index: int,
    scan_active: threading.Event | None,
    scan_start: float | None = None,
) -> dict:
    """Fire storm ops; return timings and counts.

    `scan_active` cleared stops issuing new ops (in-flight ops run out
    under their own timeout). None means no concurrent scan (storm-idle).
    """
    profile = cfg.workload
    op_kind = config.storm_profile_kind(profile)
    if op_kind is None:
        raise WorkloadError(f"{profile}: not a storm profile")
    concurrent = scan_active is not None
    requested_total = cfg.storm_clients * cfg.storm_ops
    minimum_total = storm_minimum_total(cfg.storm_clients, cfg.storm_ops, concurrent)

    min_rows = (
        min(cfg.expected_entries or minimum_total, max(8, cfg.storm_clients // 8))
        if concurrent
        else min(cfg.expected_entries or minimum_total, 8)
    )
    if min_rows < 1:
        min_rows = 1
    lines = _wait_for_rows(cfg, client, logger, scan_active, min_rows=min_rows)
    rows = [tuple(line.split("\t", 1)) for line in lines]

    timestamp = _prefetch_timestamp(cfg.out)
    token = _prefetch_token(cfg.out)
    if timestamp is None or token is None:
        code, body, _ = client.post("/get/prefetch", {})
        if code != 200:
            raise WorkloadError("storm: prefetch for timestamp failed")
        if cfg.out is not None:
            (cfg.out / "prefetch.json").write_bytes(body)
        doc = _body_json(body)
        if isinstance(doc, dict):
            timestamp = doc.get("prefetch", {}).get("timestamp")
            token = doc.get("token")
    if timestamp is None:
        raise WorkloadError("storm: no prefetch timestamp")

    results: list = []
    results_lock = threading.Lock()
    counter = itertools.count()
    started_all = time.perf_counter()
    active_wall = None

    def one_op(client_index: int) -> None:
        nonlocal active_wall
        while True:
            if scan_active is not None and not scan_active.is_set():
                if active_wall is None:
                    active_wall = time.perf_counter() - started_all
                return
            if not concurrent:
                op_index = next(counter)
                if op_index >= minimum_total:
                    return
            else:
                op_index = next(counter)
            start = time.perf_counter()
            try:
                if op_kind == "edit":
                    target_index = (client_index * cfg.storm_ops + op_index) % len(rows)
                    code, _, _ = client.put(
                        "/put/edit_rating",
                        {"indexArray": [target_index], "timestamp": timestamp, "rating": 3},
                        timeout=30.0,
                    )
                elif op_kind == "thumb":
                    hash_value, token = random.choice(rows)
                    code, _, _ = client.get(
                        _compressed_path(hash_value), bearer=token, timeout=30.0
                    )
                else:  # read
                    code, _, _ = client.post("/get/prefetch", {}, timeout=30.0)
            except WorkloadError:
                code = "error"
            elapsed = time.perf_counter() - start
            with results_lock:
                results.append((op_index, client_index, elapsed, code))
            emit_event(
                cfg.out,
                "storm_sample",
                profile=profile,
                client=client_index,
                op=op_index,
                latency_ms=elapsed * 1000.0,
                code=code,
            )

    logger.log(
        f"=== storm {profile} (run {run_index}): "
        f"{cfg.storm_clients} clients x {cfg.storm_ops} ops on {len(rows)} rows ==="
    )
    with futures.ThreadPoolExecutor(max_workers=cfg.storm_clients) as pool:
        submitted = [pool.submit(one_op, client_index) for client_index in range(cfg.storm_clients)]
        for future in futures.as_completed(submitted):
            future.result()
    wall = time.perf_counter() - started_all
    if active_wall is None:
        active_wall = wall

    latencies = sorted(elapsed for _, _, elapsed, _ in results)
    failed = sum(1 for _, _, _, code in results if code != 200)
    scan_end = None
    coverage_pct = None
    if scan_start is not None and active_wall is not None:
        scan_end = time.perf_counter()
        scan_wall = scan_end - scan_start
        if scan_wall > 0:
            coverage_pct = min(100.0, 100.0 * active_wall / scan_wall)
    summary = {
        "profile": profile,
        "kind": op_kind,
        "done": len(results),
        "total": minimum_total,
        "requested_total": requested_total,
        "median": statistics.median(latencies) if latencies else 0.0,
        "p95": _percentile(latencies, 95),
        "max": max(latencies) if latencies else 0.0,
        "failed": failed,
        "wall": wall,
        "active_wall": active_wall,
        "end": started_all + wall,
        "coverage_pct": coverage_pct,
    }
    path = round_dir / f"storm-round-{run_index}.tsv"
    with open(path, "w", encoding="utf-8") as handle:  # noqa: PTH123
        for op_index, client_index, elapsed, code in sorted(results):
            handle.write(f"{op_index}\t{client_index}\t{elapsed!r}\t{code}\n")
    limit = f" (minimum capped from {requested_total})" if minimum_total != requested_total else ""
    logger.log(
        f"storm: profile={profile} ops={summary['done']}/{minimum_total}{limit} "
        f"failed={failed} wall={wall:.1f}s kind={op_kind} active_wall={active_wall:.1f}s"
    )
    emit_event(
        cfg.out,
        "storm_summary",
        profile=profile,
        kind=op_kind,
        done=summary["done"],
        total=minimum_total,
        median=summary["median"],
        p95=summary["p95"],
        max=summary["max"],
        failed=failed,
        wall=wall,
        active_wall=active_wall,
        coverage_pct=coverage_pct,
        median_ms=summary["median"] * 1000.0,
        p95_ms=summary["p95"] * 1000.0,
        max_ms=summary["max"] * 1000.0,
        wall_ms=wall * 1000.0,
        active_wall_ms=active_wall * 1000.0,
    )
    return summary


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
    """Poll status briefly so threads do not start against stale data."""
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
                round_dir=None,
                out=None,
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


def run_scans(cfg: config.WorkloadConfig, client: HttpClient, logger: Logger) -> float:
    out = cfg.out
    walls = []
    walls_path = out / "scan-walls.tsv"
    walls_path.write_text("")
    cell_probe_rows = []
    probe_outcomes = []
    profile = cfg.workload
    # Scans repeat to reach the probe-round budget; a profile with no probe
    # phase has nothing to accumulate, so one scan per repetition.
    scan_repeats = cfg.scan_repeats if config.profile_has_probes(profile) else 1
    is_idle = config.is_idle_storm(profile)
    for run_index in range(1, scan_repeats + 1):
        logger.log(f"=== triggering index scan (run {run_index}/{cfg.scan_repeats}) ===")
        started = time.perf_counter()
        start_ms = int(time.time() * 1000)
        code, body, _ = client.post("/post/index/album", {"album": "/"})
        (out / "index-trigger.json").write_bytes(body)
        if code != 202:
            raise WorkloadError(f"index trigger returned {code}")
        logger.log(f"index trigger=202 start_ms={start_ms}")

        (out / "scan-done.marker").unlink(missing_ok=True)
        round_dir = out / f"rounds-run-{run_index}"
        round_dir.mkdir(parents=True, exist_ok=True)

        scan_active = threading.Event()
        scan_active.set()
        storm_result: dict | None = None
        storm_error: Exception | None = None

        storm_guard_result: dict | None = None

        def run_storm_guarded() -> None:
            nonlocal storm_result, storm_error, storm_guard_result
            guard = ScanPhase(
                out,
                "storm",
                expected_state="running",
                run=run_index,
                round_no=run_index,
                allow_terminal_end=True,
            )
            guard.start()
            try:
                storm_result = run_storm(
                    cfg, client, logger, round_dir, run_index, scan_active, scan_start=started
                )
            except Exception as err:  # noqa: BLE001 - storm failure must not hang the cell
                storm_error = err
            finally:
                storm_guard_result = guard.end()
                if storm_guard_result is not None and not storm_guard_result.get("valid", False):
                    reason = (
                        f"storm guard invalid: {storm_guard_result.get('reason')} "
                        f"(states={storm_guard_result.get('state_start')}->{storm_guard_result.get('state_end')}, "
                        f"scanned={storm_guard_result.get('scanned_start')}->{storm_guard_result.get('scanned_end')}, "
                        f"processed={storm_guard_result.get('processed_start')}->{storm_guard_result.get('processed_end')})"
                    )
                    if storm_error is None:
                        storm_error = WorkloadError(reason)
                    else:
                        storm_error = WorkloadError(f"{storm_error}: {reason}")

        probe_outcome = {}

        def _run_probe() -> None:
            nonlocal probe_outcome
            probe_outcome = (
                probe_phase(cfg, client, logger, round_dir, cached_rows=cell_probe_rows) or {}
            )

        refreshed_state = _refresh_scan_status(cfg, client, out, logger)
        terminal_after_warmup = False
        if (
            refreshed_state not in TERMINAL_STATUS_STATES
            and run_index == 1
            and cfg.scan_warmup > 0
            and config.profile_has_probes(profile)
        ):
            warmup_result = warmup_scan(cfg, client, out, run_index, cell_probe_rows)
            logger.log(
                f"warmup: valid={warmup_result['valid']} "
                f"reason={warmup_result['reason']} wall_ms={warmup_result['wall_ms']}"
            )
            if not warmup_result["valid"]:
                logger.log(f"warn: warmup invalid: {warmup_result['reason']}")
            if _status_state(out) in TERMINAL_STATUS_STATES or (out / "scan-done.marker").is_file():
                logger.log("warn: scan terminal after warmup")
                terminal_after_warmup = True

        if refreshed_state in TERMINAL_STATUS_STATES or terminal_after_warmup:
            logger.log(
                f"scan run {run_index}/{cfg.scan_repeats}: already terminal at refresh ({refreshed_state})"
            )
            probe_thread = None
            storm_thread = None
        elif config.profile_has_probes(profile):
            probe_thread = threading.Thread(target=_run_probe, daemon=True)
            storm_thread = None
            probe_thread.start()
        elif not is_idle and config.storm_profile_parts(profile) is not None:
            probe_thread = None
            storm_thread = threading.Thread(target=run_storm_guarded, daemon=True)
            storm_thread.start()
        else:  # storm-idle: populate first, storm the tree at rest afterwards
            probe_thread = None
            storm_thread = None

        logger.log("=== polling /get/index/status ===")
        if cfg.scan_timeout_s > 0:
            logger.log(f"scan poll budget: {cfg.scan_timeout_s:g}s (explicit SCAN_TIMEOUT_S)")
        else:
            logger.log(
                "scan poll budget: none — waiting for a terminal state (completed/failed/canceled)"
            )
        deadline = time.monotonic() + cfg.scan_timeout_s if cfg.scan_timeout_s > 0 else None
        polls = 0
        poll_started = time.monotonic()
        last_signature = None
        last_progress = time.monotonic()
        stalled = False
        state = "unknown"
        while True:
            try:
                code, body, _ = client.get("/get/index/status")
                if code == 200:
                    (out / "status-last.json").write_bytes(body)
            except WorkloadError:
                logger.log("status poll failed")
            state = _status_state(out)
            if state == "unknown":
                document = {}
            else:
                document = _body_json((out / "status-last.json").read_bytes()) or {}
            signature = tuple(
                document.get(key) for key in ("scanned", "matched", "processed", "failed")
            )
            if signature != last_signature:
                last_signature = signature
                last_progress = time.monotonic()
            polls += 1
            if state in ("completed", "failed", "canceled"):
                break
            if polls % 1500 == 0:
                logger.log(
                    f"still polling: state={state} polls={polls} "
                    f"elapsed={int(time.monotonic() - poll_started)}s"
                )
            if (
                cfg.scan_stall_timeout_s > 0
                and time.monotonic() - last_progress >= cfg.scan_stall_timeout_s
            ):
                stalled = True
                logger.log(
                    f"scan stalled: no counter movement for "
                    f"{cfg.scan_stall_timeout_s:g}s state={state} polls={polls} "
                    f"counters={signature}"
                )
                break
            if deadline is not None and time.monotonic() >= deadline:
                break
            time.sleep(cfg.poll_interval_s)

        wall_ms = int((time.perf_counter() - started) * 1000)
        scan_active.clear()
        t_terminal = time.perf_counter()
        _write_status_final(out, run_index)
        (out / "scan-done.marker").write_text(f"state={state} wall_ms={wall_ms}\n")
        walls.append(wall_ms)
        with open(walls_path, "a", encoding="utf-8") as handle:  # noqa: PTH123
            handle.write(f"{run_index}\t{wall_ms}\n")
        logger.log(
            f"scan run {run_index}/{cfg.scan_repeats}: state={state} "
            f"polls={polls} wall_ms={wall_ms}"
        )
        emit_event(
            out,
            "scan_state",
            run=run_index,
            state=state,
            polls=polls,
            wall_ms=wall_ms,
        )
        excerpt_path = out / "status-final.json"
        if not excerpt_path.is_file():
            excerpt_path = out / "status-last.json"
        final_status = _body_json(excerpt_path.read_bytes()) or {}
        logger.log(
            "status excerpt: "
            + json.dumps(
                {
                    key: final_status.get(key)
                    for key in (
                        "state",
                        "scanned",
                        "matched",
                        "processed",
                        "failed",
                        "startedAt",
                        "finishedAt",
                    )
                },
                separators=(",", ":"),
            )
        )

        if probe_thread is not None:
            probe_thread.join(timeout=60)
            logger.log(f"probe phase transcript: {out / 'probe-phase.log'}")
            rounds_total = (
                probe_outcome.get("rounds_total", 0) if isinstance(probe_outcome, dict) else 0
            )
            rounds_valid = (
                probe_outcome.get("rounds_valid", 0) if isinstance(probe_outcome, dict) else 0
            )
            progress = (
                probe_outcome.get("progress", False) if isinstance(probe_outcome, dict) else False
            )
            logger.log(
                f"probe phase outcome: rounds_total={rounds_total} "
                f"rounds_valid={rounds_valid} progress={progress}"
            )
            if not progress:
                logger.log("warn: probe phase made no counter progress")
            if rounds_valid == 0 and (
                cell_probe_rows
                or any(isinstance(o, dict) and o.get("rounds_valid", 0) > 0 for o in probe_outcomes)
            ):
                logger.log("warn: probe phase had zero valid rounds for this repeat")
            probe_outcomes.append(probe_outcome if isinstance(probe_outcome, dict) else {})

        if storm_thread is not None:
            storm_thread.join(timeout=120)
            if storm_error is not None:
                raise WorkloadError(f"storm failed: {storm_error}")
            if storm_result is None:
                raise WorkloadError("storm produced no result")

        if state != "completed":
            suffix = " after stall" if stalled else ""
            raise WorkloadError(f"scan did not complete (state={state}){suffix}")

        def is_edit_storm() -> bool:
            return config.storm_profile_access(profile) == "edit"

        if storm_thread is not None and storm_result is not None:
            check_storm_coverage(storm_result, started, t_terminal, cfg.storm_min_coverage_pct)
            if is_edit_storm():
                verify_storm_sample(cfg, client, logger, out, run_index)

    if is_idle:
        idle_guard = ScanPhase(
            out, "storm-idle", expected_state="completed", run=scan_repeats, round_no=scan_repeats
        )
        idle_guard.start()
        try:
            run_storm(cfg, client, logger, out / f"rounds-run-{scan_repeats}", scan_repeats, None)
        finally:
            idle_result = idle_guard.end()
        if not idle_result.get("valid", False):
            raise WorkloadError(
                f"{cfg.workload} idle guard invalid: {idle_result.get('reason')} "
                f"(state={idle_result.get('state_start')}->{idle_result.get('state_end')})"
            )
        logger.log(f"{cfg.workload}: no coverage check (no concurrent scan)")
        if config.storm_profile_access(cfg.workload) == "edit":
            verify_storm_sample(cfg, client, logger, out, scan_repeats)

    if probe_outcomes:
        total_valid = sum(
            (o.get("rounds_valid", 0) if isinstance(o, dict) else 0) for o in probe_outcomes
        )
        per_repeat = ", ".join(
            str((o.get("rounds_valid", 0) if isinstance(o, dict) else 0)) for o in probe_outcomes
        )
        logger.log(f"probe phase cell: valid_rounds={total_valid} per_repeat=[{per_repeat}]")
        if total_valid < cfg.min_valid_rounds:
            raise WorkloadError(
                f"probe phase: cell valid rounds {total_valid} < "
                f"min_valid_rounds={cfg.min_valid_rounds} (per_repeat=[{per_repeat}])"
            )

    median_ms = statistics.median(walls)
    logger.log(
        f"scan wall median_ms={median_ms:g} over {len(walls)} run(s): "
        f"{','.join(str(wall) for wall in walls)}"
    )

    code, body, _ = client.get("/get/index/status")
    live_state = (_body_json(body) or {}).get("state", "unknown") if code == 200 else "unknown"
    if live_state != "completed":
        raise WorkloadError(
            f"refusing post-scan work: live state={live_state} (expected completed)"
        )
    logger.log("scan terminally completed — starting post-scan phase")
    return median_ms, len(walls)


# --- post-scan ---------------------------------------------------------------
def run_sequential_probes(cfg: config.WorkloadConfig, client: HttpClient, lines: list) -> None:
    path = cfg.out / "probe-seq.tsv"
    records = []
    for line in lines:
        hash_value, token = line.split("\t", 1)
        try:
            code, _, elapsed = client.get(_compressed_path(hash_value), bearer=token)
            records.append(f"{hash_value}\t{code}\t{elapsed:.6f}")
        except WorkloadError:
            records.append(f"{hash_value}\tcurl_error\t0")
    path.write_text("\n".join(records) + ("\n" if records else ""))


def probe_stats(path: Path, label: str) -> str:
    rows = []
    try:
        text = path.read_text()
    except OSError:
        text = ""
    for line in text.splitlines():
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        try:
            elapsed = float(parts[2])
        except ValueError:
            elapsed = 0.0
        rows.append((parts[1], elapsed))
    if not rows:
        return f"{label}: EMPTY"
    times = [elapsed for _, elapsed in rows]
    ok = sum(1 for code, _ in rows if code == "200")
    return f"{label}: n={len(rows)} ok={ok} max={max(times):.4f}"


def _pick_first_servable(cfg: config.WorkloadConfig, client: HttpClient, lines: list) -> tuple:
    for line in lines[:8]:
        hash_value, token = line.split("\t", 1)
        try:
            code, _, _ = client.get(_compressed_path(hash_value), bearer=token)
        except WorkloadError:
            continue
        if code == 200:
            return hash_value, token
    raise WorkloadError("no servable thumbnail for post-scan hyperfine")


def run_post_scan(cfg: config.WorkloadConfig, client: HttpClient, logger: Logger) -> None:
    out = cfg.out
    logger.log("=== final sweep (sequential, post-scan) ===")
    lines = build_probe_list(cfg, client)
    if lines is None:
        raise WorkloadError("could not build probe list after scan")
    # Scroll probe at rest (post-settle), mirroring the during-scan probe.
    try:
        timestamp = _prefetch_timestamp(out)
        token = _prefetch_token(out)
        if timestamp is not None and token is not None:
            scroll_result = scroll_probe(
                cfg,
                client,
                timestamp,
                token,
                total_rows=len(lines) if lines else None,
                out=out,
                phase="post",
            )
            emit_event(
                cfg.out,
                "scroll_probe",
                phase="post",
                background="idle",
                scrollbar_ms=[value * 1000.0 for value in scroll_result["scrollbar"]],
                rows_ms=[value * 1000.0 for value in scroll_result["rows"]],
                data_ms=[value * 1000.0 for value in scroll_result["data"]],
            )
    except Exception:  # noqa: BLE001
        logger.log("scroll probe (post) failed; continuing")
    run_sequential_probes(cfg, client, lines)
    seq_path = out / "probe-seq.tsv"
    if not seq_path.is_file() or not seq_path.read_text():
        raise WorkloadError("no sequential probe timings")
    logger.log(f"final sweep: {probe_stats(seq_path, 'seq')}")
    logger.log(f"probe artifacts: {seq_path} {len(lines)} rows")

    first_hash, first_token = _pick_first_servable(cfg, client, lines)
    timestamp = _prefetch_timestamp(out)
    if timestamp is None:
        raise WorkloadError("no prefetch timestamp for edit-rating probe")

    benches = [
        ("compressed-get", cmd_compressed(cfg, first_hash, first_token)),
        ("prefetch", cmd_prefetch(cfg)),
        ("edit-rating", cmd_edit_rating(cfg, timestamp)),
    ]

    logger.log(f"=== hyperfine (warmup={cfg.warmup} runs={cfg.repeats}) ===")
    results = run_hyperfine(
        cfg,
        warmup=cfg.warmup,
        runs=cfg.repeats,
        export_json=out / "hyperfine.json",
        benches=benches,
        logger=logger,
    )
    if len(results) != 3:
        raise WorkloadError("hyperfine json has no results")

    logger.log("=== write-probe verification (rating == 3 on snapshot index 0) ===")
    lines = build_probe_list(cfg, client)
    if lines is None:
        raise WorkloadError("write-probe verification: could not rebuild probe list")
    verify_timestamp = _prefetch_timestamp(out)
    verify_token = _prefetch_token(out)
    if verify_timestamp is None or verify_token is None:
        raise WorkloadError("write-probe verification: no prefetch timestamp/token")

    code, body, _ = client.get(
        f"/get/get-data?timestamp={verify_timestamp}&start=0&end=1",
        bearer=verify_token,
    )
    (out / "verify-row.json").write_bytes(body)
    if code != 200:
        raise WorkloadError(f"write-probe verification: get-data returned {code} (expected 200)")
    rows = _body_json(body)
    asset = rows[0].get("assetId") if isinstance(rows, list) and rows else None
    if not asset:
        raise WorkloadError("write-probe verification: no row at snapshot index 0")

    code, body, _ = client.get(
        f"/get/metadata/{asset}?timestamp={verify_timestamp}", bearer=verify_token
    )
    (out / "verify-metadata.json").write_bytes(body)
    if code != 200:
        raise WorkloadError(
            f"write-probe verification: get-metadata returned {code} (expected 200)"
        )
    metadata = _body_json(body) or {}
    rating = metadata.get("rating")
    if rating != 3:
        shown = rating if rating is not None else "<unset>"
        raise WorkloadError(
            f"write-probe verification FAILED: rating={shown} on {asset} (expected 3)"
        )
    logger.log(f"write-probe verification: asset {asset} rating={rating} (expected 3)")
    emit_event(
        cfg.out,
        "verify_result",
        kind="write_probe",
        asset=asset,
        rating=rating,
        expected=3,
        matched=1 if rating == 3 else 0,
        total=1,
    )

    logger.log("=== summary ===")
    logger.log(f"N={cfg.ncpu} image_count={cfg.image_count} image_bytes={cfg.image_bytes}")
    logger.log(
        f"scan_wall_ms={cfg.scan_wall_ms:g} scan_state=completed (median of {cfg.scan_runs} run(s))"
    )
    logger.log(probe_stats(seq_path, "final-sweep"))
    logger.log(f"artifacts: {out}")
    logger.log("done")


# --- upload profile ----------------------------------------------------------
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


def run_upload_stimulus(cfg: config.WorkloadConfig, client: HttpClient, logger: Logger) -> None:
    sources = _select_upload_sources(cfg)
    logger.log(f"=== upload stimulus: {len(sources)} files ===")
    submit_times = []
    visible_times = []
    poll_interval = cfg.poll_interval_s
    timeout = cfg.upload_timeout_s
    post_timeout = cfg.upload_post_timeout_s
    for source in sources:
        started = time.perf_counter()
        status, body, elapsed = client.upload("/post/upload", source, timeout=post_timeout)
        submit_ms = int(elapsed * 1000)
        if status != 200:
            raise WorkloadError(f"upload failed for {source.name}: HTTP {status}")
        doc = _body_json(body)
        files = doc.get("files") if isinstance(doc, dict) else None
        stored_name = (
            files[0]
            if isinstance(files, list) and files and isinstance(files[0], str)
            else source.name
        )
        deadline = time.monotonic() + timeout
        visible = False
        while time.monotonic() < deadline:
            if _uploaded_row_visible(cfg, client, stored_name):
                visible = True
                break
            time.sleep(poll_interval)
        if not visible:
            raise WorkloadError(
                f"upload verification FAILED: file={stored_name} never became visible"
            )
        visible_ms = int((time.perf_counter() - started) * 1000)
        logger.log(f"upload file={stored_name} submit_ms={submit_ms} visible_ms={visible_ms}")
        emit_event(
            cfg.out,
            "upload_file",
            file=stored_name,
            submit_ms=submit_ms,
            visible_ms=visible_ms,
        )
        submit_times.append(submit_ms)
        visible_times.append(visible_ms)

    submit_p50 = int(statistics.median(submit_times)) if submit_times else 0
    visible_p50 = int(statistics.median(visible_times)) if visible_times else 0
    logger.log(f"upload submit_p50={submit_p50} visible_p50={visible_p50}")
    emit_event(
        cfg.out,
        "upload_summary",
        submit_p50=submit_p50,
        visible_p50=visible_p50,
    )


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


def run_workload(cfg: config.WorkloadConfig) -> int:
    prepare_out(cfg)
    logger = Logger(cfg.out / "harness-a.log")
    try:
        logger.log("=== workload preflight ===")
        logger.log(
            f"base_url={cfg.base_url} out={cfg.out} "
            f"scan_repeats={cfg.scan_repeats} repeats={cfg.repeats} "
            f"warmup={cfg.warmup} runs_during={cfg.runs_during} "
            f"fetches={cfg.fetches} images={cfg.images}"
        )
        logger.log(f"tool hyperfine: {cfg.hyperfine or 'MISSING'}")

        image_count, image_bytes = prepare_images(cfg, logger)
        cfg.image_count = image_count
        cfg.image_bytes = image_bytes
        cfg.expected_entries = image_count

        logger.log("=== run parameters ===")
        logger.log(f"N={cfg.ncpu} out={cfg.out}")
        logger.log(f"image_count={image_count} image_bytes={image_bytes}")
        logger.log(
            f"hyperfine={cfg.hyperfine} warmup={cfg.warmup} "
            f"repeats={cfg.repeats} runs_during={cfg.runs_during} "
            f"scan_repeats={cfg.scan_repeats} build_profile={cfg.build_profile} "
            f"port={cfg.port}"
        )
        logger.log(f"scan repeats: {cfg.scan_repeats} (median wall time reported)")

        client = HttpClient(cfg.base_url, cfg.cookie_jar)
        if cfg.workload == "upload":
            wait_for_index_settled(cfg, client, logger)
            run_upload_stimulus(cfg, client, logger)
            cfg.expected_entries = cfg.image_count + cfg.upload_files
        median_ms, run_count = run_scans(cfg, client, logger)
        cfg.scan_wall_ms = median_ms
        cfg.scan_runs = run_count
        if config.profile_has_probes(cfg.workload):
            post_guard = ScanPhase(cfg.out, "post-scan", expected_state="completed")
            post_guard.start()
            try:
                run_post_scan(cfg, client, logger)
            finally:
                post_result = post_guard.end()
            if not post_result.get("valid", False):
                raise WorkloadError(
                    f"post-scan guard invalid: {post_result.get('reason')} "
                    f"(state={post_result.get('state_start')}->{post_result.get('state_end')})"
                )
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
