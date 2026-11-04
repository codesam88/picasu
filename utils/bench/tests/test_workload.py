#!/usr/bin/env python3
"""Tests for the stdout-free workload wrapper. Stdlib only.

Runs the real CLI against an in-process stub origin that emulates the
prefetch/get-data/compressed/index-status/edit-rating/metadata endpoints.
Hyperfine is replaced by a fake script emitting hyperfine-2-shaped JSON,
so no real server, curl, or hyperfine binary is required.
"""

import contextlib
import http.cookiejar
import importlib.util
import io
import json
import os
import stat
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

BENCH = Path(__file__).resolve().parent.parent
EXTRACT = BENCH / "extract-bench.py"
RUN_WORKLOAD = BENCH / "run-workload.py"
SNAPSHOT_TS = 1_700_000_000


def load_workload():
    spec = importlib.util.spec_from_file_location("workload", BENCH / "workload.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class StubOrigin(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, *args, **kwargs):
        super().__init__(*args, RequestHandlerClass=StubHandler, **kwargs)
        self.mode = "completed"  # completed | stuck
        self.total_rows = 4
        self.run_duration = 1.2
        self.apply_rating = True
        self.started_at = None
        self.trigger_count = 0
        self.rating = 0
        self.requests = []

    def reset_run(self):
        self.started_at = time.monotonic()
        self.trigger_count += 1
        self.rating = 0

    def visible_rows(self):
        if self.started_at is None:
            return 0
        if self.mode == "stuck":
            return 1
        elapsed = time.monotonic() - self.started_at
        if elapsed >= self.run_duration:
            return self.total_rows
        return min(self.total_rows, int((elapsed / self.run_duration) * self.total_rows))

    def status(self):
        if self.started_at is None:
            state, rows = "idle", 0
        elif self.mode == "stuck":
            state, rows = "running", 1
        elif time.monotonic() - self.started_at >= self.run_duration:
            state, rows = "completed", self.total_rows
        else:
            state, rows = "running", self.visible_rows()
        return {
            "state": state,
            "scanned": rows,
            "matched": rows,
            "processed": rows,
            "failed": 0,
            "startedAt": SNAPSHOT_TS * 1000,
            "finishedAt": None,
        }


def image_row(index):
    return {
        "abstractData": {"type": "image", "id": f"hash{index:04d}"},
        "timestamp": SNAPSHOT_TS,
        "token": f"tok-{index}",
        "assetId": f"asset-{index}",
    }


class StubHandler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        return

    def _send(self, code, payload):
        body = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _parts(self):
        split = urlsplit(self.path)
        return split.path, parse_qs(split.query)

    def do_GET(self):
        path, query = self._parts()
        self.server.requests.append(("GET", path, query))
        if path == "/get/server-info":
            self._send(200, {})
        elif path == "/get/index/status":
            self._send(200, self.server.status())
        elif path == "/get/get-data":
            start = int(query.get("start", ["0"])[0])
            end = int(query.get("end", ["1000"])[0])
            visible = self.server.visible_rows()
            end = min(end, visible)
            rows = [] if start >= end else [image_row(i) for i in range(start, end)]
            self._send(200, rows)
        elif path.startswith("/object/compressed/"):
            name = path.rsplit("/", 1)[-1]
            hash_part = name[:-4] if name.endswith(".jpg") else name
            known = {f"hash{i:04d}" for i in range(self.server.total_rows)}
            if hash_part in known:
                self._send(200, b"JPEGDATA")
            else:
                self._send(404, {})
        elif path.startswith("/get/metadata/"):
            asset = path.rsplit("/", 1)[-1]
            rating = self.server.rating if asset == "asset-0" else 0
            self._send(200, {"rating": rating})
        else:
            self._send(404, {})

    def do_POST(self):
        path, query = self._parts()
        length = int(self.headers.get("Content-Length", "0"))
        self.rfile.read(length)
        self.server.requests.append(("POST", path, query))
        if path == "/get/prefetch":
            self._send(200, {"prefetch": {"timestamp": SNAPSHOT_TS}, "token": "snapshot-token"})
        elif path == "/post/index/album":
            self.server.reset_run()
            self._send(202, {})
        else:
            self._send(404, {})

    def do_PUT(self):
        path, query = self._parts()
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length)
        self.server.requests.append(("PUT", path, query))
        if path == "/put/edit_rating":
            body = json.loads(raw or b"{}")
            if self.server.apply_rating and body.get("rating") == 3:
                self.server.rating = 3
            self._send(200, {})
        else:
            self._send(404, {})


FAKE_HYPERFINE = r"""#!/usr/bin/env python3
import json
import re
import sys
import urllib.request

args = sys.argv[1:]
out = None
runs = 1
names = []
commands = []
i = 0
while i < len(args):
    arg = args[i]
    if arg == "--warmup":
        i += 2
    elif arg in ("-r", "--runs"):
        runs = int(args[i + 1])
        i += 2
    elif arg == "--export-json":
        out = args[i + 1]
        i += 2
    elif arg in ("-n", "--command-name"):
        names.append(args[i + 1])
        i += 2
    else:
        commands.append(arg)
        i += 1

# Emulate the one side effect the workload later verifies: hyperfine runs the
# edit-rating curl command, which applies rating 3 to snapshot index 0.
for command in commands:
    if "/put/edit_rating" not in command:
        continue
    url = re.search(r"(http://[^\s']+)", command)
    body = re.search(r"-d '(\{.*?\})'", command)
    if url and body:
        request = urllib.request.Request(
            url.group(1), data=body.group(1).encode(),
            headers={"Content-Type": "application/json"}, method="PUT",
        )
        urllib.request.urlopen(request).read()

results = []
for index, name in enumerate(names):
    base = 0.010 + index * 0.001
    results.append({
        "name": name,
        "command": name,
        "summary": {
            "time_wall_clock": {
                "mean": round(base, 4),
                "median": round(base - 0.001, 4),
                "stddev": 0.001,
                "count": runs,
            }
        },
    })
with open(out, "w") as handle:
    json.dump({"results": results}, handle)
print("fake hyperfine", len(results))
"""


def write_fake_hyperfine(directory: Path) -> Path:
    path = directory / "fake-hyperfine"
    path.write_text(FAKE_HYPERFINE)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


def make_cookie_jar(path: Path) -> None:
    jar = http.cookiejar.MozillaCookieJar(str(path))
    cookie = http.cookiejar.Cookie(
        version=0,
        name="jwt",
        value="stub-token",
        port=None,
        port_specified=False,
        domain="127.0.0.1",
        domain_specified=False,
        domain_initial_dot=False,
        path="/",
        path_specified=True,
        secure=False,
        expires=None,
        discard=False,
        comment=None,
        comment_url=None,
        rest={"HttpOnly": None},
        rfc2109=False,
    )
    jar.set_cookie(cookie)
    jar.save(ignore_discard=True, ignore_expires=True)


def make_images(tmp: Path, count: int):
    source = tmp / "source-images"
    source.mkdir()
    entries = []
    for i in range(1, count + 1):
        name = f"bench_{i:04d}.jpg"
        (source / name).write_bytes(b"x" * 100_000)
        entries.append(
            {
                "output": name,
                "format": "jpeg",
                "width": 4000,
                "height": 3000,
                "exif_date": f"2024:06:{(i % 28) + 1:02d} {(i - 1) % 24:02d}:15:00",
                "tags": ["bench", "harness-a"],
            }
        )
    manifest = tmp / "manifest.json"
    manifest.write_text(json.dumps(entries))
    return source, manifest


class WorkloadTest(unittest.TestCase):
    def setUp(self):
        self.server = StubOrigin(("127.0.0.1", 0))
        self.port = self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self._stop_server)
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.tmp_path = Path(self.tmp.name)
        self.cell = self.tmp_path / "cell"
        self.cell.mkdir()
        make_cookie_jar(self.cell / "cookies.txt")
        self.launcher_info = self.cell / "launcher-info.json"
        self.launcher_info.write_text(
            json.dumps(
                {
                    "base_url": f"http://127.0.0.1:{self.port}",
                    "cookie_jar": str(self.cell / "cookies.txt"),
                    "pid": 1234,
                    "out": str(self.cell),
                    "mode": "bench",
                }
            )
        )
        self.fake_hyperfine = write_fake_hyperfine(self.tmp_path)
        self.images, self.manifest = make_images(self.tmp_path, 4)

    def _stop_server(self):
        self.server.shutdown()
        self.server.server_close()

    def env(self, **overrides):
        env = os.environ.copy()
        env.update(
            {
                "LAUNCHER_INFO": str(self.launcher_info),
                "COUNT": "4",
                "SCAN_REPEATS": "1",
                "REPEATS": "2",
                "WARMUP": "0",
                "RUNS_DURING": "2",
                "FETCHES": "2",
                "REUSE_IMAGES": str(self.images),
                "MANIFEST": str(self.manifest),
                "HYPERFINE_BIN": str(self.fake_hyperfine),
                "SCAN_TIMEOUT_S": "5",
                "POLL_INTERVAL_S": "0.02",
                "PROBE_INTERVAL_S": "0.1",
            }
        )
        env.pop("OUT", None)
        env.update(overrides)
        return env

    def run_cli(self, env):
        return subprocess.run(
            [sys.executable, str(RUN_WORKLOAD)],
            env=env,
            capture_output=True,
            text=True,
            timeout=40,
        )

    def assert_cell_artifact(self, name):
        path = self.cell / name
        self.assertTrue(path.exists(), f"missing artifact {name}")
        return path

    def test_full_workload_completes_and_extractor_parses(self):
        proc = self.run_cli(self.env())
        self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)

        for name in (
            "harness-a.log",
            "status-final.json",
            "status-final-run-1.json",
            "hyperfine.json",
            "scan-walls.tsv",
            "prefetch.json",
            "get-data.json",
            "probe-list.tsv",
            "probe-seq.tsv",
            "verify-row.json",
            "verify-metadata.json",
        ):
            self.assert_cell_artifact(name)
        self.assert_cell_artifact("rounds-run-1/hyperfine-round-1.json")
        self.assert_cell_artifact("rounds-run-1/probe-round-1.tsv")

        hf = json.loads((self.cell / "hyperfine.json").read_text())
        self.assertEqual(
            [r["name"] for r in hf["results"]], ["compressed-get", "prefetch", "edit-rating"]
        )
        self.assertEqual(json.loads((self.cell / "verify-metadata.json").read_text())["rating"], 3)
        self.assertEqual(len((self.cell / "probe-seq.tsv").read_text().splitlines()), 4)
        self.assertEqual(
            json.loads((self.cell / "status-final.json").read_text())["state"], "completed"
        )
        self.assertEqual(
            (self.cell / "status-final.json").read_bytes(),
            (self.cell / "status-final-run-1.json").read_bytes(),
        )
        prefetch = json.loads((self.cell / "prefetch.json").read_text())
        self.assertIn("timestamp", prefetch["prefetch"])
        self.assertIn("token", prefetch)
        rows = json.loads((self.cell / "get-data.json").read_text())
        self.assertEqual(rows[0]["abstractData"]["type"], "image")
        for line in (self.cell / "probe-list.tsv").read_text().splitlines():
            self.assertEqual(len(line.split("\t")), 2)
        self.assertTrue((self.cell / "verify-row.json").is_file())
        self.assertIn("assetId", json.loads((self.cell / "verify-row.json").read_text())[0])
        walls = (self.cell / "scan-walls.tsv").read_text().strip().split("\t")
        self.assertEqual(walls[0], "1")
        self.assertGreater(int(walls[1]), 0)

        log = (self.cell / "harness-a.log").read_text()
        self.assertIn("scan wall median_ms=", log)
        self.assertIn("probe round=", log)
        self.assertIn("hyperfine compressed-get: mean=", log)

        extract = subprocess.run(
            [sys.executable, str(EXTRACT), str(self.cell)],
            capture_output=True,
            text=True,
        )
        self.assertEqual(extract.returncode, 0, extract.stderr)
        lines = [line for line in extract.stdout.splitlines() if line.strip()]
        self.assertIn("scan_fetch", lines[0])
        self.assertIn("post_edit", lines[0])
        self.assertNotIn("n/a", lines[1])

    def test_stuck_scan_exits_nonzero_with_partial_artifacts(self):
        self.server.mode = "stuck"
        proc = self.run_cli(self.env(SCAN_TIMEOUT_S="0", SCAN_STALL_TIMEOUT_S="0.5"))
        self.assertNotEqual(proc.returncode, 0, proc.stdout)
        self.assert_cell_artifact("status-last.json")
        # A running snapshot is not terminal: it keeps the -last name only.
        self.assertFalse((self.cell / "status-final.json").exists())
        self.assertFalse((self.cell / "status-final-run-1.json").exists())
        self.assert_cell_artifact("scan-walls.tsv")
        self.assertTrue((self.cell / "rounds-run-1").is_dir())
        self.assertFalse((self.cell / "hyperfine.json").exists())
        log = (self.cell / "harness-a.log").read_text()
        self.assertIn("scan stalled:", log)
        self.assertIn("scan run 1/1:", log)
        self.assertIn("scan did not complete", log)

    def test_write_probe_verification_failure(self):
        self.server.apply_rating = False
        proc = self.run_cli(self.env())
        self.assertNotEqual(proc.returncode, 0, proc.stdout)
        self.assert_cell_artifact("hyperfine.json")
        self.assert_cell_artifact("verify-metadata.json")
        rating = json.loads((self.cell / "verify-metadata.json").read_text())["rating"]
        self.assertNotEqual(rating, 3)
        log = (self.cell / "harness-a.log").read_text()
        self.assertIn("write-probe verification FAILED", log)


class ImageSetTest(unittest.TestCase):
    def test_generation_delegates_to_snapfab(self):
        workload = load_workload()
        fake = (
            "#!/usr/bin/env python3\n"
            "import json, sys\n"
            "entries = json.load(open(sys.argv[-1]))\n"
            "for entry in entries:\n"
            "    open(entry['output'], 'wb').write(b'x' * 1000)\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            snapfab = tmp_path / "fake-snapfab"
            snapfab.write_text(fake)
            snapfab.chmod(snapfab.stat().st_mode | stat.S_IXUSR)
            out = tmp_path / "cell"
            out.mkdir()
            for jobs in (1, 2):
                with self.subTest(jobs=jobs):
                    for stale in (out / "images").glob("*.jpg"):
                        stale.unlink()
                    logger = workload.Logger(out / f"gen-{jobs}.log")
                    try:
                        cfg = {
                            "out": out,
                            "manifest": None,
                            "count": 4,
                            "reuse_images": None,
                            "skip_generate": False,
                            "gen_jobs": jobs,
                            "snapfab": str(snapfab),
                        }
                        with contextlib.redirect_stdout(io.StringIO()):
                            count, size = workload.prepare_images(cfg, logger)
                    finally:
                        logger.close()
                    self.assertEqual(count, 4)
                    self.assertEqual(size, 4000)
                    self.assertEqual(len(list((out / "images").glob("*.jpg"))), 4)
                    self.assertTrue((out / "manifest.json").is_file())
                    text = (out / f"gen-{jobs}.log").read_text()
                    self.assertIn("generated=4 bytes=4000", text)
                    self.assertIn("parallel" if jobs > 1 else "sequential", text)


class ConfigTest(unittest.TestCase):
    def test_config_from_env_reads_launcher_info_and_env(self):
        workload = load_workload()
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            cell = tmp_path / "cell"
            cell.mkdir()
            make_cookie_jar(cell / "cookies.txt")
            info = cell / "launcher-info.json"
            info.write_text(
                json.dumps(
                    {
                        "base_url": "http://127.0.0.1:9999",
                        "cookie_jar": str(cell / "cookies.txt"),
                        "pid": 1,
                        "out": str(cell),
                        "mode": "bench",
                    }
                )
            )
            env = {
                "LAUNCHER_INFO": str(info),
                "COUNT": "7",
                "SCAN_REPEATS": "2",
                "REPEATS": "4",
                "WARMUP": "1",
                "RUNS_DURING": "2",
                "FETCHES": "3",
                "SCAN_TIMEOUT_S": "0",
            }
            cfg = workload.config_from_env(env)
            self.assertEqual(cfg["base_url"], "http://127.0.0.1:9999")
            self.assertEqual(cfg["out"], cell.resolve())
            self.assertEqual(cfg["count"], 7)
            self.assertEqual(cfg["scan_repeats"], 2)
            self.assertEqual(cfg["repeats"], 4)
            self.assertEqual(cfg["warmup"], 1)
            self.assertEqual(cfg["runs_during"], 2)
            self.assertEqual(cfg["fetches"], 3)

    def test_reuse_images_does_not_require_snapfab(self):
        workload = load_workload()
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            cell = tmp_path / "cell"
            cell.mkdir()
            make_cookie_jar(cell / "cookies.txt")
            info = cell / "launcher-info.json"
            info.write_text(
                json.dumps(
                    {
                        "base_url": "http://127.0.0.1:9999",
                        "cookie_jar": str(cell / "cookies.txt"),
                        "pid": 1,
                        "out": str(cell),
                        "mode": "bench",
                    }
                )
            )
            images = tmp_path / "images"
            images.mkdir()
            (images / "bench_0001.jpg").write_bytes(b"x")
            env = {
                "LAUNCHER_INFO": str(info),
                "REUSE_IMAGES": str(images),
                "SNAPFAB_BIN": str(tmp_path / "does-not-exist"),
            }
            cfg = workload.config_from_env(env)
            self.assertEqual(cfg["reuse_images"], images)
            self.assertIsNone(cfg["manifest"])


class StatusFinalTest(unittest.TestCase):
    """`status-final*` names are reserved for terminal scan states."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name)
        self.workload = load_workload()

    def write_last(self, state):
        (self.out / "status-last.json").write_text(json.dumps({"state": state}))

    def test_running_snapshot_is_not_final(self):
        self.write_last("running")
        self.workload._write_status_final(self.out, 1)
        self.assertTrue((self.out / "status-last.json").is_file())
        self.assertFalse((self.out / "status-final.json").exists())
        self.assertFalse((self.out / "status-final-run-1.json").exists())

    def test_terminal_snapshot_is_final(self):
        for state in ("completed", "failed", "canceled"):
            with self.subTest(state=state):
                self.write_last(state)
                self.workload._write_status_final(self.out, 1)
                self.assertEqual(
                    json.loads((self.out / "status-final.json").read_text())["state"], state
                )
                self.assertEqual(
                    json.loads((self.out / "status-final-run-1.json").read_text())["state"],
                    state,
                )

    def test_running_snapshot_removes_stale_final(self):
        self.write_last("completed")
        self.workload._write_status_final(self.out, 1)
        self.assertTrue((self.out / "status-final-run-1.json").is_file())

        self.write_last("running")
        self.workload._write_status_final(self.out, 2)
        self.assertFalse((self.out / "status-final.json").exists())
        self.assertFalse((self.out / "status-final-run-2.json").exists())
        self.assertTrue((self.out / "status-final-run-1.json").is_file())


if __name__ == "__main__":
    unittest.main()
