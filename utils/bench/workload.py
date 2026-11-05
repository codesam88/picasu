#!/usr/bin/env python3
"""Workload half of a benchmark cell, against a launcher-owned server.

Consumes `launcher-info.json` written by `utils/bench/backend_launcher.py`
and implements the measurement protocol (line formats compatible with
the retired shell driver, so historical cells keep parsing):
image-set materialisation, repeated album-index scans, during-scan random
compressed fetches plus hyperfine prefetch/edit-rating probes, a post-scan
sequential sweep, post-scan hyperfine baselines, and write-probe verification.

Artifacts are written in the harness-A layout so `utils/bench/extract-bench.py`
parses either producer unchanged:

  harness-a.log                    run transcript (harness line formats)
  status-last.json                 most recent /get/index/status body
  status-final.json                last run's terminal status
  status-final-run-<n>.json        per-run terminal status
  scan-walls.tsv                   run index + wall ms
  prefetch.json / get-data.json    raw probe-list sources
  probe-list.tsv                   hash<TAB>token rows
  probe-seq.tsv                    hash<TAB>code<TAB>seconds sweep
  rounds-run-<n>/                  per-run probe rounds
    hyperfine-round-<n>.json
    hyperfine-round-<n>.out
    probe-round-<n>.tsv
  hyperfine.json                   post-scan hyperfine export
  verify-row.json / verify-metadata.json
  probe-phase.log                  during-scan transcript

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
                  per-round warmup is always 0, matching harness-A)
  SCAN_REPEATS    scan repetitions per cell (default 3)
  FETCHES         random compressed fetches per during-scan round (default 10)
  PROBE_LIMIT     max probe-list rows (default: IMAGES)
  SCAN_TIMEOUT_S  scan poll budget; 0 waits for a terminal state (default 0)
  SCAN_STALL_TIMEOUT_S  abort when running counters do not move for this
                  long; 0 disables (default 120)
  POLL_INTERVAL_S status poll interval (default 0.2)
  PROBE_INTERVAL_S probe-phase refresh interval (default 0.5)
  IMAGES is required to be present in IMAGE_REPO: repos are seeded
  a priori (see `just bench seed-images`); the workload never generates.
  WORKLOAD        control (default) | storm-diff | storm-read | storm-idle:
                  control runs probe rounds during the scan; storm-diff fires
                  concurrent edit-rating PUTs at distinct assets, storm-read
                  concurrent prefetch POSTs, storm-idle runs one populate scan
                  then storms the tree at rest (no margin check)
  STORM_CLIENTS   concurrent storm clients (default 8)
  STORM_OPS       ops per storm client (default 10)
  STORM_MARGIN_S  storm must finish this long before the scan ends
                  (default 5); on violation the run fails naming the
                  scantime increase factor
  HYPERFINE_BIN   hyperfine binary; empty disables hyperfine

Exit codes: 0 completed; 1 config/preflight; 2 runtime failure (including a
scan that did not reach `completed` and a failed write-probe verification).
"""

from __future__ import annotations

import argparse
import concurrent.futures
import http.cookiejar
import itertools
import json
import math
import os
import random
import re
import shutil
import statistics
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent

DEFAULT_IMAGES = 100
DEFAULT_REPEATS = 3
DEFAULT_WARMUP = 1
DEFAULT_RUNS_DURING = 3
DEFAULT_SCAN_REPEATS = 3
DEFAULT_FETCHES = 10
DEFAULT_STORM_CLIENTS = 8
DEFAULT_STORM_OPS = 10
DEFAULT_STORM_MARGIN_S = 5.0
WORKLOADS = ("control", "storm-diff", "storm-read", "storm-idle")
DEFAULT_POLL_INTERVAL_S = 0.2
DEFAULT_PROBE_INTERVAL_S = 0.5
DEFAULT_BUILD_PROFILE = "debug"
DEFAULT_PORT = 5713
TERMINAL_STATUS_STATES = ("completed", "failed", "canceled")

ARTIFACT_FILES = (
    "harness-a.log",
    "status-last.json",
    "status-final.json",
    "scan-walls.tsv",
    "scan-done.marker",
    "prefetch.json",
    "get-data.json",
    "probe-list.tsv",
    "probe-seq.tsv",
    "probe-phase.log",
    "hyperfine.json",
    "hyperfine-fallback.txt",
    "index-trigger.json",
    "verify-row.json",
    "verify-metadata.json",
)
ARTIFACT_GLOBS = ("rounds-run-*", "status-final-run-*.json")


class ConfigError(Exception):
    """Invalid or unsafe workload configuration."""


class WorkloadError(Exception):
    """Runtime failure after the cell started."""


# --- configuration -----------------------------------------------------------
def load_launcher_info(path: Path) -> dict:
    try:
        info = json.loads(path.read_text())
    except OSError as err:
        raise ConfigError(f"cannot read launcher info {path}: {err}")
    except json.JSONDecodeError as err:
        raise ConfigError(f"invalid launcher info {path}: {err}")
    for key in ("base_url", "cookie_jar", "out"):
        if not info.get(key):
            raise ConfigError(f"launcher info missing '{key}': {path}")
    return info


def _int_env(environ: dict, name: str, default: int, minimum: int = 0) -> int:
    raw = environ.get(name, str(default))
    try:
        value = int(raw)
    except ValueError:
        raise ConfigError(f"{name} must be an integer, got '{raw}'")
    if value < minimum:
        raise ConfigError(f"{name} must be >= {minimum}, got {value}")
    return value


def _float_env(environ: dict, name: str, default: float, minimum: float = 0.0) -> float:
    raw = environ.get(name, str(default))
    try:
        value = float(raw)
    except ValueError:
        raise ConfigError(f"{name} must be a number, got '{raw}'")
    if value < minimum:
        raise ConfigError(f"{name} must be >= {minimum}, got {value}")
    return value


def resolve_hyperfine(environ: dict) -> str:
    explicit = environ.get("HYPERFINE_BIN")
    if explicit is not None:
        if explicit == "":
            return ""
        if not (Path(explicit).is_file() and os.access(explicit, os.X_OK)):
            raise ConfigError(f"HYPERFINE_BIN is not executable: {explicit}")
        return explicit
    found = shutil.which("hyperfine")
    if found:
        return found
    fallback = Path("/tmp/opencode/worker-pool-rework/bin/hyperfine")
    if fallback.is_file() and os.access(fallback, os.X_OK):
        return str(fallback)
    return ""


def _validate_out(raw: str) -> Path:
    if raw == "":
        raise ConfigError("OUT is required")
    out = Path(os.path.realpath(raw))
    if str(out) == "/":
        raise ConfigError("refusing to use OUT='/'")
    if out == REPO_ROOT or REPO_ROOT in out.parents:
        raise ConfigError(f"OUT must not be inside the repo: {out}")
    return out


def config_from_env(environ: dict) -> dict:
    for old, new in (
        ("N", "NCPU"),
        ("COUNT", "IMAGES"),
        ("REUSE_IMAGES", "IMAGE_REPO"),
        ("SKIP_GENERATE", None),
        ("GEN_JOBS", None),
        ("SNAPFAB_BIN", None),
        ("MANIFEST", None),
    ):
        if old in environ:
            hint = f"use {new} instead" if new else "seeding moved to `just bench seed-images`"
            raise ConfigError(f"{old} was removed ({hint})")

    if not environ.get("IMAGE_REPO", ""):
        raise ConfigError("IMAGE_REPO is required (seed a repo with `just bench seed-images`)")

    info_path = environ.get("LAUNCHER_INFO", "")
    if not info_path:
        raise ConfigError("LAUNCHER_INFO is required")
    info = load_launcher_info(Path(info_path))

    base_url = str(info["base_url"]).rstrip("/")
    cookie_jar = Path(info["cookie_jar"])
    if not cookie_jar.is_file():
        raise ConfigError(f"cookie jar not found: {cookie_jar}")

    out = _validate_out(environ.get("OUT") or str(info["out"]))

    try:
        port = urllib.parse.urlsplit(base_url).port or DEFAULT_PORT
    except ValueError:
        raise ConfigError(f"invalid base_url: {base_url}")

    ncpu = _int_env(environ, "NCPU", 4, minimum=0)

    build_profile = environ.get("BUILD_PROFILE", DEFAULT_BUILD_PROFILE)
    if build_profile not in ("debug", "release"):
        raise ConfigError(f"BUILD_PROFILE must be debug or release, got '{build_profile}'")

    images = _int_env(environ, "IMAGES", DEFAULT_IMAGES, minimum=1)
    workload = environ.get("WORKLOAD", "control")
    if workload not in WORKLOADS:
        raise ConfigError(f"WORKLOAD must be one of {WORKLOADS}, got '{workload}'")
    storm_clients = _int_env(environ, "STORM_CLIENTS", DEFAULT_STORM_CLIENTS, minimum=1)
    storm_ops = _int_env(environ, "STORM_OPS", DEFAULT_STORM_OPS, minimum=1)
    storm_margin = _float_env(environ, "STORM_MARGIN_S", DEFAULT_STORM_MARGIN_S, minimum=0.0)
    scan_repeats = _int_env(environ, "SCAN_REPEATS", DEFAULT_SCAN_REPEATS, minimum=1)
    repeats = _int_env(environ, "REPEATS", DEFAULT_REPEATS, minimum=1)
    warmup = _int_env(environ, "WARMUP", DEFAULT_WARMUP, minimum=0)
    runs_during = _int_env(environ, "RUNS_DURING", DEFAULT_RUNS_DURING, minimum=1)
    fetches = _int_env(environ, "FETCHES", DEFAULT_FETCHES, minimum=0)
    probe_limit = _int_env(environ, "PROBE_LIMIT", images, minimum=1)
    scan_timeout = _float_env(environ, "SCAN_TIMEOUT_S", 0.0, minimum=0.0)
    scan_stall_timeout = _float_env(environ, "SCAN_STALL_TIMEOUT_S", 120.0, minimum=0.0)
    poll_interval = _float_env(environ, "POLL_INTERVAL_S", DEFAULT_POLL_INTERVAL_S, minimum=0.001)
    probe_interval = _float_env(
        environ, "PROBE_INTERVAL_S", DEFAULT_PROBE_INTERVAL_S, minimum=0.001
    )

    repo_path = Path(environ["IMAGE_REPO"])
    if not repo_path.is_dir():
        raise ConfigError(f"IMAGE_REPO not a directory: {environ['IMAGE_REPO']}")

    image_path = Path(environ.get("IMAGE_PATH", "") or out / "images")

    return {
        "launcher_info": info,
        "base_url": base_url,
        "cookie_jar": cookie_jar,
        "out": out,
        "ncpu": ncpu,
        "port": port,
        "images": images,
        "workload": workload,
        "storm_clients": storm_clients,
        "storm_ops": storm_ops,
        "storm_margin_s": storm_margin,
        "scan_repeats": scan_repeats,
        "repeats": repeats,
        "warmup": warmup,
        "runs_during": runs_during,
        "fetches": fetches,
        "probe_limit": probe_limit,
        "scan_timeout_s": scan_timeout,
        "scan_stall_timeout_s": scan_stall_timeout,
        "poll_interval_s": poll_interval,
        "probe_interval_s": probe_interval,
        "image_repo": repo_path,
        "image_path": image_path,
        "build_profile": build_profile,
        "hyperfine": resolve_hyperfine(environ),
    }


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

    def raw(self, text: str) -> None:
        if not text:
            return
        with self.lock:
            self.handle.write(text if text.endswith("\n") else text + "\n")
            self.handle.flush()

    def close(self) -> None:
        self.handle.close()


class ProbeTranscript:
    """Mirror of probe-phase log lines, as harness-A's redirected stdout."""

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


def prepare_images(cfg: dict, logger: Logger) -> tuple:
    img_dir = cfg["image_path"]
    img_dir.mkdir(parents=True, exist_ok=True)

    available = sorted(cfg["image_repo"].glob("*.jpg"), key=lambda path: path.name)
    if len(available) < cfg["images"]:
        raise ConfigError(
            f"IMAGE_REPO holds {len(available)} images, need IMAGES={cfg['images']}: "
            f"seed more with `just bench seed-images` (repo: {cfg['image_repo']})"
        )
    wanted = available[: cfg["images"]]

    logger.log(f"=== seeding images (repo={cfg['image_repo']} path={img_dir}) ===")
    started = time.perf_counter()
    for stale in img_dir.glob("*.jpg"):
        stale.unlink()
    for source in wanted:
        shutil.copy2(source, img_dir / source.name)
    logger.log(f"seeded {len(wanted)} images from {cfg['image_repo']} (source untouched)")
    mode = f"seeded({cfg['image_repo']})"
    wall_ms = int((time.perf_counter() - started) * 1000)

    jpgs = list(img_dir.glob("*.jpg"))
    image_count = len(jpgs)
    image_bytes = sum(path.stat().st_size for path in jpgs)
    if image_count != cfg["images"]:
        raise WorkloadError(f"expected {cfg['images']} images, found {image_count}")
    logger.log(f"generated={image_count} bytes={image_bytes} wall_ms={wall_ms} mode={mode}")
    return image_count, image_bytes


# --- probe list --------------------------------------------------------------
def build_probe_list(cfg: dict, client: HttpClient) -> list | None:
    out = cfg["out"]
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

    code, body, _ = client.get(
        f"/get/get-data?timestamp={timestamp}&start=0&end=1000", bearer=token
    )
    (out / "get-data.json").write_bytes(body)
    if code != 200:
        return None
    rows = _body_json(body)
    if not isinstance(rows, list):
        return None
    lines = []
    for row in rows:
        abstract = row.get("abstractData", {})
        if abstract.get("type") != "image":
            continue
        hash_value = abstract.get("id")
        row_token = row.get("token")
        if hash_value is None or row_token is None:
            continue
        lines.append(f"{hash_value}\t{row_token}")
        if len(lines) >= cfg["probe_limit"]:
            break
    text = "\n".join(lines) + ("\n" if lines else "")
    (out / "probe-list.tsv").write_text(text)
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


# --- hyperfine ---------------------------------------------------------------
def cmd_prefetch(cfg: dict) -> str:
    return (
        f"curl -s -o /dev/null -b {cfg['cookie_jar']} -X POST "
        f"-H 'Content-Type: application/json' -d '{{}}' "
        f"-w '%{{http_code}} %{{time_total}}\\n' {cfg['base_url']}/get/prefetch"
    )


def cmd_compressed(cfg: dict, hash_value: str, token: str) -> str:
    return (
        f"curl -s -o /dev/null -b {cfg['cookie_jar']} "
        f"-H 'Authorization: Bearer {token}' "
        f"-w '%{{http_code}} %{{time_total}}\\n' "
        f"{cfg['base_url']}{_compressed_path(hash_value)}"
    )


def cmd_edit_rating(cfg: dict, timestamp) -> str:
    return (
        f"curl -s -o /dev/null -b {cfg['cookie_jar']} -X PUT "
        f"-H 'Content-Type: application/json' "
        f'-d \'{{"indexArray":[0],"timestamp":{timestamp},"rating":3}}\' '
        f"-w '%{{http_code}} %{{time_total}}\\n' {cfg['base_url']}/put/edit_rating"
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
    return result.get("summary", {}).get("time_wall_clock", {})


def run_hyperfine(
    cfg: dict,
    warmup: int,
    runs: int,
    export_json: Path,
    benches: list,
    logger: Logger,
    output_path: Path | None = None,
) -> list:
    binary = cfg["hyperfine"]
    if not binary:
        return []
    command = [binary, "--warmup", str(warmup), "-r", str(runs), "--export-json", str(export_json)]
    for name, shell in benches:
        command.extend(["-n", name, shell])
    export_json.unlink(missing_ok=True)
    result = subprocess.run(command, capture_output=True, text=True)
    combined = result.stdout + result.stderr
    logger.raw(combined)
    if output_path is not None:
        output_path.write_text(combined)
    if result.returncode != 0:
        raise WorkloadError(f"hyperfine exited {result.returncode}")
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
    cfg: dict,
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
    if lines and cfg["fetches"]:
        picks = random.sample(lines, min(cfg["fetches"], len(lines)))
    records = []
    for line in picks:
        hash_value, token = line.split("\t", 1)
        try:
            code, _, elapsed = client.get(_compressed_path(hash_value), bearer=token)
        except WorkloadError:
            code, elapsed = 0, 0.0
        records.append(f"{hash_value}\t{code}\t{elapsed:.6f}")
    sample.write_text("\n".join(records) + ("\n" if records else ""))
    if records:
        times = [float(record.split("\t")[2]) for record in records]
        thumb = f"n={len(times)} mean={sum(times) / len(times):.4f}"
    else:
        thumb = "n=0"
    transcript.log(f"probe round={round_no} rows={rows} state={state} fetch: thumb: {thumb}")


def probe_round(
    cfg: dict,
    client: HttpClient,
    logger: Logger,
    transcript: ProbeTranscript,
    round_no: int,
    rows: int,
    state: str,
    lines: list,
    round_dir: Path,
) -> bool:
    out = cfg["out"]
    timestamp = _prefetch_timestamp(out)
    if timestamp is None:
        transcript.log(
            f"probe round={round_no} rows={rows} state={state} "
            "no prefetch timestamp; skipping round"
        )
        return False

    random_fetch_round(cfg, client, transcript, round_no, rows, state, lines, round_dir)

    json_path = round_dir / f"hyperfine-round-{round_no}.json"
    if cfg["hyperfine"]:
        benches = [
            ("prefetch", cmd_prefetch(cfg)),
            ("edit-rating", cmd_edit_rating(cfg, timestamp)),
        ]
        try:
            results = run_hyperfine(
                cfg,
                warmup=0,
                runs=cfg["runs_during"],
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
            return False
        if len(results) != 2:
            transcript.log(
                f"probe round={round_no} rows={rows} state={state} hyperfine json has no results"
            )
            return False
        summary = _summary_line(results)
    else:
        prefetch_time = "curl_error"
        edit_time = "curl_error"
        try:
            _, _, prefetch_time = client.post("/get/prefetch", {})
        except WorkloadError:
            pass
        try:
            _, _, edit_time = client.put(
                "/put/edit_rating",
                {"indexArray": [0], "timestamp": timestamp, "rating": 3},
            )
        except WorkloadError:
            pass
        summary = (
            f"prefetch single={prefetch_time} s | "
            f"edit-rating single={edit_time} s (hyperfine MISSING)"
        )
    transcript.log(f"probe round={round_no} rows={rows} state={state} hyperfine: {summary}")
    return True


def probe_phase(
    cfg: dict, client: HttpClient, logger: Logger, expected: int, round_dir: Path
) -> None:
    out = cfg["out"]
    transcript = ProbeTranscript(logger, out / "probe-phase.log")
    marker = out / "scan-done.marker"
    waited = 0.0
    last_rows = None
    rounds = 0
    state = "unknown"
    lines = []
    try:
        while True:
            lines = build_probe_list(cfg, client) or []
            rows = len(lines)
            state = _status_state(out)
            if rows != last_rows and rows > 0:
                rounds += 1
                try:
                    probe_round(
                        cfg,
                        client,
                        logger,
                        transcript,
                        rounds,
                        rows,
                        state,
                        lines,
                        round_dir,
                    )
                except Exception:  # noqa: BLE001 - a probe must not kill the scan
                    transcript.log(f"probe round {rounds} failed; continuing")
                last_rows = rows
            if marker.is_file():
                break
            if rows >= expected:
                break
            if cfg["scan_timeout_s"] > 0 and waited >= cfg["scan_timeout_s"]:
                transcript.log(
                    f"probe phase: rows={rows} state={state} waited~{waited:g}s "
                    "(explicit SCAN_TIMEOUT_S), probing what exists"
                )
                break
            time.sleep(cfg["probe_interval_s"])
            waited += cfg["probe_interval_s"]
        if rounds == 0:
            transcript.log(f"probe phase: no image rows before the scan settled (state={state})")
        else:
            transcript.log(
                f"probe phase: rounds={rounds} rows={len(lines)} "
                f"state_at_break={_status_state(out)}"
            )
    finally:
        transcript.close()


# --- storm workloads ---------------------------------------------------------
# storm-diff: concurrent edit-rating PUTs at distinct assets.
# storm-read: concurrent prefetch POSTs.
# Both run while a scan is active and stop issuing new ops once the scan
# reaches a terminal state; the margin check then verifies the storm
# finished early enough for the numbers to describe loaded operation.
# storm-idle runs one populate scan first, then storms the tree at rest
# (no margin check — there is no concurrent scan).


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
    cfg: dict, client: HttpClient, logger: Logger, scan_active: threading.Event | None
) -> list:
    """Poll the probe list until rows appear or the scan goes terminal."""
    for _ in range(150):  # ~30 s cap; the poll loop clears active on settle
        lines = build_probe_list(cfg, client) or []
        if lines:
            return lines
        if scan_active is not None and not scan_active.is_set():
            break
        time.sleep(0.2)
    raise WorkloadError("storm: no probe rows available (scan too short?)")


def run_storm(
    cfg: dict,
    client: HttpClient,
    logger: Logger,
    round_dir: Path,
    run_index: int,
    scan_active: threading.Event | None,
) -> dict:
    """Fire STORM_CLIENTS x STORM_OPS ops; return timings and counts.

    `scan_active` cleared stops issuing new ops (in-flight ops run out
    under their own timeout). None means no concurrent scan (storm-idle).
    """
    profile = cfg["workload"]
    total = cfg["storm_clients"] * cfg["storm_ops"]

    code, body, _ = client.post("/get/prefetch", {})
    if code != 200:
        raise WorkloadError("storm: prefetch for timestamp failed")
    timestamp = (_body_json(body) or {}).get("prefetch", {}).get("timestamp")
    if timestamp is None:
        raise WorkloadError("storm: no prefetch timestamp")
    lines = _wait_for_rows(cfg, client, logger, scan_active)
    rows = [line.split("\t", 1)[0] for line in lines]

    results: list = []
    results_lock = threading.Lock()
    counter = itertools.count()
    started_all = time.perf_counter()

    def one_op(client_index: int) -> None:
        while scan_active is None or scan_active.is_set():
            op_index = next(counter)
            if op_index >= total:
                return
            start = time.perf_counter()
            try:
                if profile == "storm-diff":
                    code, _, _ = client.put(
                        "/put/edit_rating",
                        {"indexArray": [op_index % len(rows)], "timestamp": timestamp, "rating": 3},
                        timeout=30.0,
                    )
                else:  # storm-read
                    code, _, _ = client.post("/get/prefetch", {}, timeout=30.0)
            except WorkloadError:
                code = "error"
            elapsed = time.perf_counter() - start
            with results_lock:
                results.append((op_index, client_index, elapsed, code))

    logger.log(
        f"=== storm {profile} (run {run_index}): "
        f"{cfg['storm_clients']} clients x {cfg['storm_ops']} ops on {len(rows)} rows ==="
    )
    with concurrent.futures.ThreadPoolExecutor(max_workers=cfg["storm_clients"]) as pool:
        futures = [
            pool.submit(one_op, client_index) for client_index in range(cfg["storm_clients"])
        ]
        for future in concurrent.futures.as_completed(futures):
            future.result()
    wall = time.perf_counter() - started_all

    latencies = sorted(elapsed for _, _, elapsed, _ in results)
    failed = sum(1 for _, _, _, code in results if code != 200)
    summary = {
        "profile": profile,
        "done": len(results),
        "total": total,
        "median": statistics.median(latencies) if latencies else 0.0,
        "p95": _percentile(latencies, 95),
        "max": max(latencies) if latencies else 0.0,
        "failed": failed,
        "wall": wall,
        "end": started_all + wall,
    }
    path = round_dir / f"storm-round-{run_index}.tsv"
    with open(path, "w", encoding="utf-8") as handle:  # noqa: PTH123
        for op_index, client_index, elapsed, code in sorted(results):
            handle.write(f"{op_index}\t{client_index}\t{elapsed:.6f}\t{code}\n")
    logger.log(
        f"storm: profile={profile} ops={summary['done']}/{total} "
        f"median={summary['median']:.3f}s p95={summary['p95']:.3f}s "
        f"max={summary['max']:.3f}s failed={failed} wall={wall:.1f}s"
    )
    return summary


def check_storm_margin(
    cfg: dict, logger: Logger, summary: dict, scan_start: float, scan_end: float
) -> None:
    """Require the storm to have finished with margin before scan end."""
    scan_wall = scan_end - scan_start
    if summary["done"] >= summary["total"]:
        need = summary["wall"]
    elif summary["done"] > 0:
        need = summary["wall"] / summary["done"] * summary["total"]
    else:
        need = float("inf")
    if summary["end"] + cfg["storm_margin_s"] <= scan_end:
        logger.log(
            f"storm margin ok: storm ended {scan_end - summary['end']:.1f}s "
            f"before scan end (margin {cfg['storm_margin_s']:g}s)"
        )
        return
    factor = 1.0 if scan_wall <= 0 else (need + cfg["storm_margin_s"]) / scan_wall
    factor = max(1.5, math.ceil(factor * 10) / 10)
    raise WorkloadError(
        f"storm outran the scan: storm_wall={summary['wall']:.1f}s "
        f"ops={summary['done']}/{summary['total']} scan_wall={scan_wall:.1f}s "
        f"margin={cfg['storm_margin_s']:g}s — increase scantime by factor ~{factor:g} "
        f"(more IMAGES, slower scan, or fewer STORM_OPS)"
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


def run_scans(cfg: dict, client: HttpClient, logger: Logger) -> float:
    out = cfg["out"]
    walls = []
    walls_path = out / "scan-walls.tsv"
    walls_path.write_text("")
    scan_repeats = cfg["scan_repeats"]
    if cfg["workload"] == "storm-idle" and scan_repeats != 1:
        logger.log(f"storm-idle: forcing SCAN_REPEATS=1 (was {scan_repeats})")
        scan_repeats = 1
    for run_index in range(1, scan_repeats + 1):
        logger.log(f"=== triggering index scan (run {run_index}/{cfg['scan_repeats']}) ===")
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

        profile = cfg["workload"]
        scan_active = threading.Event()
        scan_active.set()
        storm_result: dict | None = None
        storm_error: Exception | None = None

        def run_storm_guarded() -> None:
            nonlocal storm_result, storm_error
            try:
                storm_result = run_storm(cfg, client, logger, round_dir, run_index, scan_active)
            except Exception as err:  # noqa: BLE001 - storm failure must not hang the cell
                storm_error = err

        if profile == "control":
            probe_thread = threading.Thread(
                target=probe_phase,
                args=(cfg, client, logger, cfg["expected_entries"], round_dir),
                daemon=True,
            )
            storm_thread = None
            probe_thread.start()
        elif profile in ("storm-diff", "storm-read"):
            probe_thread = None
            storm_thread = threading.Thread(target=run_storm_guarded, daemon=True)
            storm_thread.start()
        else:  # storm-idle: populate first, storm the tree at rest afterwards
            probe_thread = None
            storm_thread = None

        logger.log("=== polling /get/index/status ===")
        if cfg["scan_timeout_s"] > 0:
            logger.log(f"scan poll budget: {cfg['scan_timeout_s']:g}s (explicit SCAN_TIMEOUT_S)")
        else:
            logger.log(
                "scan poll budget: none — waiting for a terminal state (completed/failed/canceled)"
            )
        deadline = time.monotonic() + cfg["scan_timeout_s"] if cfg["scan_timeout_s"] > 0 else None
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
                cfg["scan_stall_timeout_s"] > 0
                and time.monotonic() - last_progress >= cfg["scan_stall_timeout_s"]
            ):
                stalled = True
                logger.log(
                    f"scan stalled: no counter movement for "
                    f"{cfg['scan_stall_timeout_s']:g}s state={state} polls={polls} "
                    f"counters={signature}"
                )
                break
            if deadline is not None and time.monotonic() >= deadline:
                break
            time.sleep(cfg["poll_interval_s"])

        wall_ms = int((time.perf_counter() - started) * 1000)
        scan_active.clear()
        t_terminal = time.perf_counter()
        _write_status_final(out, run_index)
        (out / "scan-done.marker").write_text(f"state={state} wall_ms={wall_ms}\n")
        walls.append(wall_ms)
        with open(walls_path, "a", encoding="utf-8") as handle:  # noqa: PTH123
            handle.write(f"{run_index}\t{wall_ms}\n")
        logger.log(
            f"scan run {run_index}/{cfg['scan_repeats']}: state={state} "
            f"polls={polls} wall_ms={wall_ms}"
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
        if storm_thread is not None:
            storm_thread.join(timeout=120)
            if storm_error is not None:
                raise WorkloadError(f"storm failed: {storm_error}")
            if storm_result is None:
                raise WorkloadError("storm produced no result")

        if state != "completed":
            suffix = " after stall" if stalled else ""
            raise WorkloadError(f"scan did not complete (state={state}){suffix}")

        if profile == "storm-idle":
            run_storm(cfg, client, logger, round_dir, run_index, None)
            logger.log("storm-idle: no margin check (no concurrent scan)")
        elif storm_thread is not None and storm_result is not None:
            check_storm_margin(cfg, logger, storm_result, started, t_terminal)

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
    return median_ms


# --- post-scan ---------------------------------------------------------------
def run_sequential_probes(cfg: dict, client: HttpClient, lines: list) -> None:
    path = cfg["out"] / "probe-seq.tsv"
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
    return (
        f"{label}: n={len(rows)} ok={ok} mean={sum(times) / len(times):.6f} "
        f"min={min(times):.6f} max={max(times):.6f}"
    )


def _pick_first_servable(cfg: dict, client: HttpClient, lines: list) -> tuple:
    for line in lines[:8]:
        hash_value, token = line.split("\t", 1)
        try:
            code, _, _ = client.get(_compressed_path(hash_value), bearer=token)
        except WorkloadError:
            continue
        if code == 200:
            return hash_value, token
    raise WorkloadError("no servable thumbnail for post-scan hyperfine")


def _redact(command: str) -> str:
    return re.sub(r"Bearer [A-Za-z0-9._-]+", "Bearer <redacted>", command)


def run_post_scan(cfg: dict, client: HttpClient, logger: Logger) -> None:
    out = cfg["out"]
    logger.log("=== final sweep (sequential, post-scan) ===")
    lines = build_probe_list(cfg, client)
    if lines is None:
        raise WorkloadError("could not build probe list after scan")
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

    logger.log(f"=== hyperfine (warmup={cfg['warmup']} runs={cfg['repeats']}) ===")
    if cfg["hyperfine"]:
        results = run_hyperfine(
            cfg,
            warmup=cfg["warmup"],
            runs=cfg["repeats"],
            export_json=out / "hyperfine.json",
            benches=benches,
            logger=logger,
        )
        if len(results) != 3:
            raise WorkloadError("hyperfine json has no results")
        for result in results:
            wall = _wall_clock(result)
            logger.log(
                f"hyperfine {_result_name(result)}: mean={wall.get('mean')} s "
                f"median={wall.get('median')} s stddev={wall.get('stddev')} "
                f"runs={wall.get('count')} cmd={_redact(str(result.get('command', '')))}"
            )
    else:
        fallback = out / "hyperfine-fallback.txt"
        fallback.write_text("")
        for run_index in range(1, cfg["repeats"] + 1):
            try:
                _, _, elapsed = client.get(_compressed_path(first_hash), bearer=first_token)
                raw = str(elapsed)
            except WorkloadError:
                raw = "curl_error"
            line = f"run={run_index} compressed_get_s={raw}"
            logger.log(line)
            with open(fallback, "a", encoding="utf-8") as handle:  # noqa: PTH123
                handle.write(line + "\n")
        logger.log(f"hyperfine MISSING: wrote {fallback}")

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

    logger.log("=== summary ===")
    logger.log(f"N={cfg['ncpu']} image_count={cfg['image_count']} image_bytes={cfg['image_bytes']}")
    logger.log(
        f"scan_wall_ms={cfg['scan_wall_ms']:g} scan_state=completed "
        f"(median of {cfg['scan_repeats']} run(s))"
    )
    logger.log(probe_stats(seq_path, "final-sweep"))
    logger.log(f"artifacts: {out}")
    logger.log("done")


# --- main flow ---------------------------------------------------------------
def prepare_out(cfg: dict) -> None:
    out = cfg["out"]
    out.mkdir(parents=True, exist_ok=True)
    for name in ARTIFACT_FILES:
        (out / name).unlink(missing_ok=True)
    for pattern in ARTIFACT_GLOBS:
        for path in out.glob(pattern):
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink(missing_ok=True)


def run_workload(cfg: dict) -> int:
    prepare_out(cfg)
    logger = Logger(cfg["out"] / "harness-a.log")
    try:
        logger.log("=== workload preflight ===")
        logger.log(
            f"base_url={cfg['base_url']} out={cfg['out']} "
            f"scan_repeats={cfg['scan_repeats']} repeats={cfg['repeats']} "
            f"warmup={cfg['warmup']} runs_during={cfg['runs_during']} "
            f"fetches={cfg['fetches']} images={cfg['images']}"
        )
        logger.log(f"tool hyperfine: {cfg['hyperfine'] or 'MISSING'}")

        image_count, image_bytes = prepare_images(cfg, logger)
        cfg["image_count"] = image_count
        cfg["image_bytes"] = image_bytes
        cfg["expected_entries"] = image_count

        logger.log("=== run parameters ===")
        logger.log(f"N={cfg['ncpu']} out={cfg['out']}")
        logger.log(f"image_count={image_count} image_bytes={image_bytes}")
        logger.log(
            f"hyperfine={cfg['hyperfine']} warmup={cfg['warmup']} "
            f"repeats={cfg['repeats']} runs_during={cfg['runs_during']} "
            f"scan_repeats={cfg['scan_repeats']} build_profile={cfg['build_profile']} "
            f"port={cfg['port']}"
        )
        logger.log(f"scan repeats: {cfg['scan_repeats']} (median wall time reported)")

        client = HttpClient(cfg["base_url"], cfg["cookie_jar"])
        cfg["scan_wall_ms"] = run_scans(cfg, client, logger)
        run_post_scan(cfg, client, logger)
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
