#!/usr/bin/env python3
"""Tests for run-cell.py. Stdlib only: python3 -m unittest -v.

The orchestrator is exercised against a stub launcher script (writes a fake
`launcher-info.json`) and a stub workload script. A local HTTP server stands
in for the backend's `/get/server-info`; no real server is started.
"""

import importlib.util
import json
import os
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from unittest import mock

BENCH = Path(__file__).resolve().parent.parent

FAKE_GDB = """\
#!/usr/bin/env python3
import os
import sys

log = os.environ.get("FAKE_GDB_LOG", "")
if log:
    with open(log, "a", encoding="utf-8") as handle:
        handle.write("gdb-attach\\n")
        handle.flush()
sys.stdout.write("Thread 1 (Thread 0x0 (LWP 1)):\\n#0  sleep ()\\n")
"""


def load_orchestrator():
    spec = importlib.util.spec_from_file_location("run_cell", BENCH / "run-cell.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


oc = load_orchestrator()

STUB_LAUNCHER = """\
import json
import os
import signal
import subprocess
import sys
import time

out = os.environ["OUT"]
os.makedirs(out, exist_ok=True)
events = os.environ.get("STUB_EVENTS", "")
server = None


def note(message):
    if not events:
        return
    with open(events, "a", encoding="utf-8") as handle:
        handle.write(message + "\\n")
        handle.flush()


def on_term(signum, frame):
    note("launcher-sigterm")
    if server is not None and server.poll() is None:
        server.terminate()
        note("server-terminated")
    sys.exit(130)


signal.signal(signal.SIGTERM, on_term)
note("launcher-start")
with open(os.path.join(out, "server.log"), "w", encoding="utf-8") as handle:
    handle.write("stub server listening\\n")
with open(os.path.join(out, "launcher-env.json"), "w", encoding="utf-8") as handle:
    json.dump(
        {k: os.environ.get(k) for k in ("OUT", "PORT", "BUILD_PROFILE", "CELL_TAG")},
        handle,
    )

mode = os.environ.get("STUB_LAUNCHER", "ready")
if mode == "die":
    note("launcher-die")
    sys.exit(2)

# Stand-in for the backend process: a plain sleeping child whose pid the
# orchestrator can inspect when diagnostics are captured live.
server = subprocess.Popen(["sleep", "300"])
if mode == "never-ready":
    while True:
        time.sleep(0.05)

info = {
    "base_url": os.environ["STUB_BASE_URL"],
    "cookie_jar": os.path.join(out, "cookies.txt"),
    "pid": server.pid,
    "out": out,
    "mode": os.environ.get("MODE", "bench"),
}
with open(os.path.join(out, "launcher-info.json"), "w", encoding="utf-8") as handle:
    json.dump(info, handle)
note("launcher-info-written")

if mode == "ignore-term":
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
while True:
    time.sleep(0.05)
"""

STUB_WORKLOAD = """\
import json
import os
import sys
import time

out = os.environ["OUT"]
events = os.environ.get("STUB_EVENTS", "")


def note(message):
    if not events:
        return
    with open(events, "a", encoding="utf-8") as handle:
        handle.write(message + "\\n")
        handle.flush()


note("workload-start")
with open(os.environ["LAUNCHER_INFO"], encoding="utf-8") as handle:
    info = json.load(handle)
note("workload-saw-info:" + info["base_url"])
with open(os.path.join(out, "workload-env.json"), "w", encoding="utf-8") as handle:
    json.dump(
        {k: os.environ.get(k) for k in ("OUT", "LAUNCHER_INFO", "IMAGES", "CELL_TAG")},
        handle,
    )


def write_status(name, state):
    body = json.dumps({"state": state, "scanned": 3, "matched": 0, "processed": 0, "failed": 0})
    with open(os.path.join(out, name), "w", encoding="utf-8") as handle:
        handle.write(body + "\\n")


mode = os.environ.get("STUB_WORKLOAD", "ok")
if mode == "hang":
    note("workload-hung")
    while True:
        time.sleep(0.05)
if mode == "fail":
    # The real workload always writes status-final.json, even when the
    # state is still running. Reproduce that so naming is tested.
    write_status("status-last.json", "running")
    write_status("status-final.json", "running")
    note("workload-failed")
    sys.exit(2)
if mode == "fail-terminal":
    write_status("status-last.json", "failed")
    write_status("status-final.json", "failed")
    note("workload-failed")
    sys.exit(2)

write_status("status-last.json", "completed")
sys.exit(0)
"""


class StubBackend(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, *args, **kwargs):
        super().__init__(*args, RequestHandlerClass=StubHandler, **kwargs)
        self.status = 200


class StubHandler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        return

    def do_GET(self):
        code = self.server.status if self.path.split("?")[0] == "/get/server-info" else 404
        body = b"{}"
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class RunCellTest(unittest.TestCase):
    def setUp(self):
        self.server = StubBackend(("127.0.0.1", 0))
        self.port = self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self._stop_server)

        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.out = root / "cell"
        self.events = root / "events.log"
        self.launcher_script = root / "stub-launcher.py"
        self.workload_script = root / "stub-workload.py"
        self.launcher_script.write_text(STUB_LAUNCHER)
        self.workload_script.write_text(STUB_WORKLOAD)
        self.fake_gdb = root / "fake-gdb.py"
        self.fake_gdb.write_text(FAKE_GDB)
        self.fake_gdb.chmod(0o755)
        self.base_url = f"http://127.0.0.1:{self.port}"

    def fake_stack_tools(self):
        """Route stack capture to a fast fake gdb and log its invocation."""
        env = mock.patch.dict(os.environ, {"FAKE_GDB_LOG": str(self.events)})
        candidates = mock.patch.object(
            oc.backend_launcher, "gdb_candidates", lambda: [str(self.fake_gdb)]
        )
        return env, candidates

    def _stop_server(self):
        self.server.shutdown()
        self.server.server_close()

    def environ(self, **kw):
        env = {
            "STUB_EVENTS": str(self.events),
            "STUB_BASE_URL": self.base_url,
            "STUB_LAUNCHER": "ready",
            "STUB_WORKLOAD": "ok",
            "IMAGES": "100",
            "CELL_TAG": "tag-1",
        }
        env.update(kw)
        return env

    def cfg(self, **kw):
        cfg = {
            "out": self.out,
            "port": self.port,
            "build_profile": "release",
            "ready_timeout": 5.0,
            "stop_timeout": 5.0,
            "workload_timeout": 0.0,
            "gdb": False,
            "poll_interval": 0.02,
            "launcher_script": self.launcher_script,
            "workload_script": self.workload_script,
        }
        cfg.update(kw)
        return cfg

    def events_list(self):
        if not self.events.is_file():
            return []
        return self.events.read_text().splitlines()

    def diag(self):
        return self.out / "diag-orchestrator"

    # --- config ---------------------------------------------------------
    def test_config_requires_out_and_port(self):
        with self.assertRaises(oc.ConfigError):
            oc.config_from_env({})
        with self.assertRaises(oc.ConfigError):
            oc.config_from_env({"OUT": str(self.out)})
        with self.assertRaises(oc.ConfigError):
            oc.config_from_env({"OUT": str(self.out), "PORT": "abc"})
        with self.assertRaises(oc.ConfigError):
            oc.config_from_env({"OUT": str(self.out), "PORT": "0"})

    def test_config_defaults_and_flags(self):
        cfg = oc.config_from_env({"OUT": str(self.out), "PORT": "5800"})
        self.assertEqual(cfg["build_profile"], "release")
        self.assertEqual(cfg["ready_timeout"], 150)
        self.assertEqual(cfg["stop_timeout"], 30)
        self.assertEqual(cfg["workload_timeout"], 0)
        self.assertTrue(cfg["gdb"])
        self.assertEqual(cfg["port"], 5800)

        cfg = oc.config_from_env(
            {"OUT": str(self.tmp.name), "PORT": "5800", "BUILD_PROFILE": "debug"},
            out_flag=str(self.out),
            port_flag="5801",
        )
        self.assertEqual(cfg["out"], Path(self.out).resolve())
        self.assertEqual(cfg["port"], 5801)
        self.assertEqual(cfg["build_profile"], "debug")

    def test_config_rejects_negative_workload_timeout(self):
        with self.assertRaises(oc.ConfigError):
            oc.config_from_env({"OUT": str(self.out), "PORT": "5800", "WORKLOAD_TIMEOUT_S": "-1"})
        cfg = oc.config_from_env(
            {"OUT": str(self.out), "PORT": "5800", "WORKLOAD_TIMEOUT_S": "0", "GDB": "0"}
        )
        self.assertEqual(cfg["workload_timeout"], 0)
        self.assertFalse(cfg["gdb"])

    def test_config_refuses_repo_out(self):
        for bad in ("/", str(oc.REPO_ROOT), str(oc.REPO_ROOT / "cell")):
            with self.assertRaises(oc.ConfigError, msg=bad):
                oc.config_from_env({"OUT": bad, "PORT": "5800"})

    # --- success --------------------------------------------------------
    def test_success_orders_children_and_passes_env(self):
        rc = oc.run_cell(self.cfg(), self.environ())
        self.assertEqual(rc, 0)

        events = self.events_list()
        self.assertIn("launcher-info-written", events)
        self.assertIn("launcher-sigterm", events)
        self.assertLess(events.index("launcher-info-written"), events.index("workload-start"))
        self.assertLess(events.index("workload-start"), events.index("launcher-sigterm"))
        self.assertIn(f"workload-saw-info:{self.base_url}", events)

        launcher_env = json.loads((self.out / "launcher-env.json").read_text())
        self.assertEqual(launcher_env["OUT"], str(self.out))
        self.assertEqual(launcher_env["PORT"], str(self.port))
        self.assertEqual(launcher_env["BUILD_PROFILE"], "release")
        self.assertNotIn("MODE", launcher_env)

        workload_env = json.loads((self.out / "workload-env.json").read_text())
        self.assertEqual(workload_env["OUT"], str(self.out))
        self.assertEqual(workload_env["LAUNCHER_INFO"], str(self.out / "launcher-info.json"))
        self.assertEqual(workload_env["IMAGES"], "100")
        self.assertEqual(workload_env["CELL_TAG"], "tag-1")

        self.assertFalse(self.diag().exists())

    # --- workload failure ----------------------------------------------
    def test_workload_failure_propagates_and_captures_live_stacks(self):
        env, which = self.fake_stack_tools()
        with env, which:
            rc = oc.run_cell(self.cfg(gdb=True), self.environ(STUB_WORKLOAD="fail"))
        self.assertEqual(rc, 2)

        # Pre-shutdown live capture: thread count and gdb stacks exist.
        threads = (self.diag() / "threads.txt").read_text().strip()
        self.assertGreaterEqual(int(threads), 1)
        self.assertTrue((self.diag() / "stacks.txt").is_file())
        self.assertIn("Thread 1", (self.diag() / "stacks.txt").read_text())

        self.assertTrue((self.diag() / "server-tail.log").is_file())
        meta = json.loads((self.diag() / "orchestrator.json").read_text())
        self.assertEqual(meta["reason"], "workload-failed")
        self.assertEqual(meta["workload_rc"], 2)
        self.assertEqual(meta["status_state"], "running")
        self.assertIsInstance(meta["server_pid"], int)

        # A running snapshot stays status-last; a terminal name is not
        # fabricated. The workload's misnamed OUT file is preserved as-is.
        self.assertTrue((self.diag() / "status-last.json").is_file())
        self.assertFalse((self.diag() / "status-final.json").exists())
        preserved = json.loads((self.out / "status-final.json").read_text())
        self.assertEqual(preserved["state"], "running")

        # Capture happens after the failure and before shutdown.
        events = self.events_list()
        self.assertLess(events.index("workload-failed"), events.index("gdb-attach"))
        self.assertLess(events.index("gdb-attach"), events.index("launcher-sigterm"))

    def test_terminal_status_is_frozen_as_status_final(self):
        rc = oc.run_cell(self.cfg(), self.environ(STUB_WORKLOAD="fail-terminal"))
        self.assertEqual(rc, 2)
        frozen = json.loads((self.diag() / "status-final.json").read_text())
        self.assertEqual(frozen["state"], "failed")
        meta = json.loads((self.diag() / "orchestrator.json").read_text())
        self.assertEqual(meta["status_state"], "failed")

    def test_live_capture_writes_proc_thread_state_without_gdb(self):
        # No gdb at all: the /proc fallback must still produce per-thread
        # state, and the metadata must record which probes succeeded.
        rc = oc.run_cell(self.cfg(gdb=False), self.environ(STUB_WORKLOAD="fail"))
        self.assertEqual(rc, 2)

        diag = self.diag()
        self.assertTrue((diag / "proc-threads.txt").is_file())
        self.assertGreaterEqual(int((diag / "threads.txt").read_text().strip()), 1)
        proc = (diag / "proc-threads.txt").read_text()
        self.assertIn("tid=", proc)
        self.assertIn("wchan=", proc)

        meta = json.loads((diag / "orchestrator.json").read_text())
        captures = meta["captures"]
        self.assertTrue(captures["proc_threads"]["ok"])
        self.assertGreaterEqual(captures["proc_threads"]["threads"], 1)
        self.assertGreaterEqual(captures["proc_threads"]["wchan"], 1)
        self.assertFalse(captures["gdb"]["ok"])
        self.assertFalse((diag / "stacks.txt").exists())

    def test_stack_capture_degrades_without_gdb(self):
        with mock.patch.object(oc.backend_launcher, "gdb_candidates", lambda: []):
            rc = oc.run_cell(self.cfg(gdb=True), self.environ(STUB_WORKLOAD="fail"))
        self.assertEqual(rc, 2)
        self.assertTrue((self.diag() / "threads.txt").is_file())
        self.assertFalse((self.diag() / "stacks.txt").exists())
        self.assertIn("launcher-sigterm", self.events_list())
        self.assertIn("stack capture skipped", (self.diag() / "stack-capture.log").read_text())
        meta = json.loads((self.diag() / "orchestrator.json").read_text())
        self.assertFalse(meta["captures"]["gdb"]["ok"])
        self.assertIn("no gdb binary found", meta["captures"]["gdb"]["reason"])

    def test_gdb_failure_is_graceful(self):
        failing_gdb = Path(self.tmp.name) / "failing-gdb.py"
        failing_gdb.write_text(
            "#!/usr/bin/env python3\nimport sys\nsys.stderr.write('cannot attach\\n')\nsys.exit(1)\n"
        )
        failing_gdb.chmod(0o755)
        with mock.patch.object(oc.backend_launcher, "gdb_candidates", lambda: [str(failing_gdb)]):
            rc = oc.run_cell(self.cfg(gdb=True), self.environ(STUB_WORKLOAD="fail"))
        self.assertEqual(rc, 2)
        self.assertTrue((self.diag() / "threads.txt").is_file())
        self.assertIn("cannot attach", (self.diag() / "stacks.txt").read_text())
        self.assertIn("launcher-sigterm", self.events_list())
        meta = json.loads((self.diag() / "orchestrator.json").read_text())
        self.assertFalse(meta["captures"]["gdb"]["ok"])
        self.assertIn("all gdb candidates failed", meta["captures"]["gdb"]["reason"])

    def test_workload_timeout_freezes_diagnostics_before_shutdown(self):
        env, which = self.fake_stack_tools()
        with env, which:
            rc = oc.run_cell(
                self.cfg(gdb=True, workload_timeout=0.4),
                self.environ(STUB_WORKLOAD="hang"),
            )
        self.assertEqual(rc, oc.EXIT_WORKLOAD_TIMEOUT)
        meta = json.loads((self.diag() / "orchestrator.json").read_text())
        self.assertEqual(meta["reason"], "workload-timeout")
        self.assertEqual(meta["workload_rc"], oc.EXIT_WORKLOAD_TIMEOUT)
        self.assertTrue((self.diag() / "threads.txt").is_file())
        self.assertTrue((self.diag() / "stacks.txt").is_file())

        events = self.events_list()
        self.assertLess(events.index("gdb-attach"), events.index("launcher-sigterm"))

    def test_workload_spawn_failure_is_orchestration_error(self):
        missing = Path(self.tmp.name) / "does-not-exist.py"
        config = self.cfg(workload_script=missing)
        with mock.patch.object(oc.subprocess, "run", side_effect=OSError("boom")):
            rc = oc.run_cell(config, self.environ())
        self.assertEqual(rc, oc.EXIT_WORKLOAD_SPAWN)
        self.assertTrue((self.diag() / "orchestrator.json").is_file())
        self.assertIn("launcher-sigterm", self.events_list())

    # --- launcher failures ---------------------------------------------
    def test_launcher_exits_before_ready(self):
        rc = oc.run_cell(self.cfg(), self.environ(STUB_LAUNCHER="die"))
        self.assertEqual(rc, oc.EXIT_LAUNCHER_NOT_READY)
        meta = json.loads((self.diag() / "orchestrator.json").read_text())
        self.assertEqual(meta["reason"], "launcher-exited")
        self.assertEqual(meta["launcher_rc"], 2)
        self.assertNotIn("workload-start", self.events_list())

    def test_launcher_never_ready_times_out_and_is_stopped(self):
        rc = oc.run_cell(
            self.cfg(ready_timeout=0.4),
            self.environ(STUB_LAUNCHER="never-ready"),
        )
        self.assertEqual(rc, oc.EXIT_LAUNCHER_NOT_READY)
        meta = json.loads((self.diag() / "orchestrator.json").read_text())
        self.assertEqual(meta["reason"], "launcher-not-ready")
        self.assertIn("launcher-sigterm", self.events_list())
        self.assertNotIn("workload-start", self.events_list())

    def test_readiness_requires_200(self):
        self.server.status = 404
        rc = oc.run_cell(
            self.cfg(ready_timeout=0.4),
            self.environ(),
        )
        self.assertEqual(rc, oc.EXIT_LAUNCHER_NOT_READY)
        self.assertNotIn("workload-start", self.events_list())
        self.assertIn("launcher-sigterm", self.events_list())

    def test_shutdown_failure_is_distinct(self):
        started = time.monotonic()
        rc = oc.run_cell(
            self.cfg(stop_timeout=0.3),
            self.environ(STUB_LAUNCHER="ignore-term"),
        )
        self.assertEqual(rc, oc.EXIT_SHUTDOWN_FAILED)
        self.assertLess(time.monotonic() - started, 10)
        self.assertIn("workload-start", self.events_list())


if __name__ == "__main__":
    unittest.main()
