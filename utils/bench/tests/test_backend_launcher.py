#!/usr/bin/env python3
"""Tests for backend_launcher.py core. Stdlib only: python3 -m unittest -v.

Covers: config validation and bench/debug guards (pure), FRESH/reuse
requirements, port preflight, and the startup/supervision paths against an
in-process stub HTTP server plus a fake server process (no real backend).
"""

import contextlib
import importlib.util
import io
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from unittest import mock

BENCH = Path(__file__).resolve().parent.parent


def load_launcher():
    spec = importlib.util.spec_from_file_location("backend_launcher", BENCH / "backend_launcher.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


bl = load_launcher()


def base_env(**kw):
    e = {
        "BUILD_PROFILE": "release",
        "PICASU_BIN": sys.executable,
        "GDB": "0",
    }
    e.update(kw)
    return e


class StubServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, *args, **kwargs):
        super().__init__(*args, RequestHandlerClass=StubHandler, **kwargs)
        self.server_info_status = 200
        self.bootstrap_status = 200
        self.calls = []


class StubHandler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        return

    def send_payload(self, code, payload, cookie=False):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        if cookie:
            self.send_header("Set-Cookie", "jwt=stub-token; Path=/; HttpOnly")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_GET(self):
        self.server.calls.append(("GET", self.path, b""))
        if self.path.split("?")[0] == "/get/server-info":
            code = self.server.server_info_status
            self.send_payload(code, b"{}")
        else:
            self.send_payload(404, b"{}")

    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        self.server.calls.append(("POST", self.path, raw))
        if self.path == "/post/users/bootstrap":
            self.send_payload(self.server.bootstrap_status, b'{"userId":"bench-admin"}')
        elif self.path == "/post/users/authenticate":
            self.send_payload(200, b"{}", cookie=True)
        else:
            self.send_payload(404, b"{}")


class FakeProc:
    """Popen stand-in backed by a real, isolated sleep process.

    A real PID keeps /proc introspection and killpg safe: the process group
    is the sleeper's own, so the launcher never touches the test runner.
    """

    def __init__(self, out, exit_after=None, exit_code=0):
        self._child = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(60)"],
            start_new_session=True,
        )
        self.pid = self._child.pid
        self.exit_after = exit_after
        self.exit_code = exit_code
        self._start = time.monotonic()
        self.returncode = None
        self._log_dir = out
        self._log_written = False

    def _ensure_log(self):
        # Written lazily: a fresh run wipes OUT after the fake proc exists.
        if not self._log_written and self._log_dir.is_dir():
            (self._log_dir / "server.log").write_text("fake server log\n")
            self._log_written = True

    def poll(self):
        self._ensure_log()
        if self.returncode is None:
            child_rc = self._child.poll()
            if child_rc is not None:
                # Killed by the launcher's group signal (negative signal code)
                # or exited on its own.
                self.returncode = child_rc
            else:
                due = self.exit_after is not None and (
                    time.monotonic() - self._start >= self.exit_after
                )
                if due:
                    self._reap(self.exit_code)
        return self.returncode

    def _reap(self, code):
        if self._child.poll() is None:
            self._child.kill()
        self._child.wait()
        self.returncode = code

    def close(self):
        if self.returncode is None:
            self._reap(0)


class LauncherTest(unittest.TestCase):
    def setUp(self):
        self.server = StubServer(("127.0.0.1", 0))
        self.port = self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self._stop_server)
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def _stop_server(self):
        self.server.shutdown()
        self.server.server_close()

    def make_cfg(self, **kw):
        env = base_env(PORT=str(self.port), OUT=str(Path(self.tmp.name) / "cell"), **kw)
        return env, bl.config_from_env(env)

    def run_cfg(self, env, proc, captured=None):
        cfg = bl.config_from_env(env)

        def spawn(cfg_, server_env_):
            if captured is not None:
                captured.update(server_env_)
            return proc

        buf = io.StringIO()
        # The stub HTTP server already listens on the configured port, which
        # is exactly what the launcher probes. Skip the probe here; the
        # dedicated busy-port test exercises the real check.
        with (
            mock.patch.object(bl, "port_in_use", return_value=False),
            contextlib.redirect_stdout(buf),
        ):
            rc = bl.run_backend(cfg, env, spawn=spawn)
        return rc, Path(env["OUT"]), buf.getvalue()

    # --- config ---------------------------------------------------------
    def test_bench_defaults(self):
        env = base_env(OUT=str(Path(self.tmp.name) / "c"))
        env.pop("GDB")
        cfg = bl.config_from_env(env)
        self.assertEqual(cfg["mode"], "bench")
        self.assertTrue(cfg["fresh"])
        self.assertEqual(cfg["build_profile"], "release")
        self.assertEqual(cfg["ready_timeout"], 120)
        self.assertEqual(cfg["launch_timeout"], 0)
        self.assertEqual(cfg["ncpu"], 4)
        self.assertTrue(cfg["gdb"])
        self.assertEqual(cfg["admin_id"], "bench-admin")
        self.assertTrue(cfg["admin_pw"].startswith("bench-"))

    def test_debug_profile_is_not_canonical(self):
        env = base_env(BUILD_PROFILE="debug", OUT=str(Path(self.tmp.name) / "c"))
        cfg = bl.config_from_env(env)
        self.assertEqual(cfg["mode"], "debug")
        self.assertEqual(cfg["build_profile"], "debug")

    def test_release_with_rust_log_is_not_canonical(self):
        env = base_env(OUT=str(Path(self.tmp.name) / "c"), RUST_LOG="info")
        cfg = bl.config_from_env(env)
        self.assertEqual(cfg["mode"], "debug")

    def test_bad_profile(self):
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(BUILD_PROFILE="prod", OUT=str(Path(self.tmp.name) / "c")))

    def test_out_refusals(self):
        for bad in ("", "/", str(bl.REPO_ROOT), str(bl.REPO_ROOT) + "/x"):
            with self.assertRaises(bl.ConfigError, msg=bad):
                bl.config_from_env(base_env(OUT=bad))

    def test_out_outside_tmp_refused(self):
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(OUT="/var/lib/picasu-launcher-test"))

    def test_mode_derived_from_profile_and_env(self):
        out = str(Path(self.tmp.name) / "c")
        cfg = bl.config_from_env(base_env(OUT=out))
        self.assertEqual(cfg["mode"], "bench")
        cfg = bl.config_from_env(base_env(OUT=out, RUST_LOG="info"))
        self.assertEqual(cfg["mode"], "debug")
        cfg = bl.config_from_env(base_env(OUT=out, BUILD_PROFILE="debug"))
        self.assertEqual(cfg["mode"], "debug")

    def test_missing_binary(self):
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(
                base_env(OUT=str(Path(self.tmp.name) / "c"), PICASU_BIN="/nonexistent")
            )

    def test_bad_port_and_fresh(self):
        out = str(Path(self.tmp.name) / "c")
        for bad in ("abc", "0", "70000"):
            with self.assertRaises(bl.ConfigError, msg=bad):
                bl.config_from_env(base_env(OUT=out, PORT=bad))
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(OUT=out, FRESH="yes"))

    def test_reuse_requires_data_and_password(self):
        out = Path(self.tmp.name) / "cell"
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(OUT=str(out), FRESH="0", ADMIN_PW="pw"))
        (out / "data").mkdir(parents=True)
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(OUT=str(out), FRESH="0"))
        cfg = bl.config_from_env(base_env(OUT=str(out), FRESH="0", ADMIN_PW="pw"))
        self.assertFalse(cfg["fresh"])

    # --- runtime --------------------------------------------------------
    def test_fresh_success(self):
        env, _ = self.make_cfg()
        out = Path(env["OUT"])
        out.mkdir(parents=True)
        (out / "stale.txt").write_text("old")
        proc = FakeProc(out, exit_after=0.6)
        self.addCleanup(proc.close)
        captured = {}
        rc, out, _ = self.run_cfg(env, proc, captured)
        self.assertEqual(rc, 0)
        self.assertFalse((out / "stale.txt").exists())
        self.assertTrue((out / "data").is_dir())
        self.assertTrue((out / "images").is_dir())
        self.assertTrue((out / "run-manifest.json").exists())
        info = json.loads((out / "launcher-info.json").read_text())
        self.assertEqual(info["base_url"], f"http://127.0.0.1:{self.port}")
        self.assertEqual(info["pid"], proc.pid)
        self.assertEqual(info["out"], str(out))
        self.assertEqual(info["mode"], "bench")
        cookies = Path(info["cookie_jar"]).read_text()
        self.assertIn("jwt", cookies)
        paths = [c[1] for c in self.server.calls]
        self.assertIn("/post/users/bootstrap", paths)
        self.assertIn("/post/users/authenticate", paths)
        self.assertEqual(captured["PICASU_DATA_HOME"], str(out / "data"))
        self.assertEqual(captured["PICASU_USERS_HOME"], str(out / "data" / "users"))
        self.assertEqual(captured["PICASU_BOOTSTRAP_ADMIN"], "true")
        self.assertEqual(captured["PICASU_PORT"], str(self.port))

    def test_reuse_skips_bootstrap(self):
        out = Path(self.tmp.name) / "cell"
        (out / "data").mkdir(parents=True)
        (out / "keep.txt").write_text("existing state")
        env = base_env(PORT=str(self.port), OUT=str(out), FRESH="0", ADMIN_PW="existing-pw")
        proc = FakeProc(out, exit_after=0.6)
        self.addCleanup(proc.close)
        captured = {}
        rc, out, _ = self.run_cfg(env, proc, captured)
        self.assertEqual(rc, 0)
        self.assertTrue((out / "keep.txt").exists())
        paths = [c[1] for c in self.server.calls]
        self.assertNotIn("/post/users/bootstrap", paths)
        self.assertIn("/post/users/authenticate", paths)
        self.assertEqual(captured["PICASU_BOOTSTRAP_ADMIN"], "false")

    def test_bootstrap_closed_is_startup_failure(self):
        self.server.bootstrap_status = 401
        env, _ = self.make_cfg()
        proc = FakeProc(Path(self.tmp.name) / "cell", exit_after=0.6)
        self.addCleanup(proc.close)
        rc, out, log = self.run_cfg(env, proc)
        self.assertEqual(rc, 2)
        self.assertIn("FRESH=0", (out / "launcher.log").read_text())
        self.assertTrue((out / "diag").is_dir())

    def test_busy_port_fails_fast(self):
        env, _ = self.make_cfg()
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            s.listen(1)
            env["PORT"] = str(s.getsockname()[1])
            env["OUT"] = str(Path(self.tmp.name) / "busy")
            started = []

            def spawn(cfg_, server_env_):
                started.append(True)
                raise AssertionError("spawn must not run on a busy port")

            cfg = bl.config_from_env(env)
            rc = bl.run_backend(cfg, env, spawn=spawn)
            self.assertEqual(rc, 1)
            self.assertEqual(started, [])
            self.assertFalse(Path(env["OUT"]).exists())
            self.assertIsNotNone(s.fileno())

    def test_server_dies_during_startup(self):
        env, _ = self.make_cfg()
        out = Path(env["OUT"])
        proc = FakeProc(Path(self.tmp.name) / "cell", exit_after=0.0, exit_code=1)
        self.addCleanup(proc.close)
        rc, out, _ = self.run_cfg(env, proc)
        self.assertEqual(rc, 2)
        self.assertTrue((out / "diag").is_dir())
        self.assertTrue((out / "diag" / "server-tail.log").exists())

    def test_readiness_timeout(self):
        self.server.server_info_status = 404
        env = base_env(
            PORT=str(self.port),
            OUT=str(Path(self.tmp.name) / "cell"),
            READY_TIMEOUT_S="0.4",
        )
        proc = FakeProc(Path(self.tmp.name) / "cell", exit_after=None)
        self.addCleanup(proc.close)
        rc, out, _ = self.run_cfg(env, proc)
        self.assertEqual(rc, 2)
        self.assertTrue((out / "diag" / "server-tail.log").exists())
        self.assertTrue((out / "diag" / "threads.txt").exists())

    def test_outer_timeout(self):
        env, _ = self.make_cfg()
        env["LAUNCH_TIMEOUT_S"] = "0.4"
        proc = FakeProc(Path(self.tmp.name) / "cell", exit_after=None)
        self.addCleanup(proc.close)
        rc, out, _ = self.run_cfg(env, proc)
        self.assertEqual(rc, 4)
        self.assertTrue((out / "diag" / "server-tail.log").exists())
        self.assertTrue((out / "diag" / "threads.txt").exists())
        self.assertTrue((out / "launcher-info.json").exists())

    def test_nonzero_exit_after_ready(self):
        env, _ = self.make_cfg()
        proc = FakeProc(Path(self.tmp.name) / "cell", exit_after=0.6, exit_code=7)
        self.addCleanup(proc.close)
        rc, out, _ = self.run_cfg(env, proc)
        self.assertEqual(rc, 2)
        self.assertTrue((out / "diag" / "launcher-info-server-exited-7.json").exists())


class StackCaptureTest(unittest.TestCase):
    """gdb candidate ordering and capture degradation behavior."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.diag = self.root / "diag"
        self.diag.mkdir()

    def script(self, name: str, body: str) -> Path:
        path = self.root / name
        path.write_text("#!/usr/bin/env python3\n" + body)
        path.chmod(0o755)
        return path

    def test_gdb_candidates_prefers_system_gdb(self):
        toolchain = self.root / "rust-gdb"
        path_gdb = self.root / "gdb"
        with (
            mock.patch.object(bl.os.path, "isfile", return_value=True),
            mock.patch.object(bl.os, "access", return_value=True),
            mock.patch.object(
                bl.shutil,
                "which",
                lambda name: str(toolchain) if name == "rust-gdb" else str(path_gdb),
            ),
        ):
            candidates = bl.gdb_candidates()
        self.assertEqual(candidates, [bl.SYSTEM_GDB, str(toolchain), str(path_gdb)])

    def test_gdb_candidates_skips_inaccessible_system_gdb(self):
        # A file-capability gdb is useless when DAC denies execute; the rustup
        # wrapper and PATH gdb stay candidates.
        toolchain = self.root / "rust-gdb"
        with (
            mock.patch.object(bl.os.path, "isfile", return_value=True),
            mock.patch.object(bl.os, "access", return_value=False),
            mock.patch.object(
                bl.shutil,
                "which",
                lambda name: str(toolchain) if name == "rust-gdb" else None,
            ),
        ):
            candidates = bl.gdb_candidates()
        self.assertEqual(candidates, [str(toolchain)])

    def test_gdb_candidates_falls_back_to_path_gdb(self):
        path_gdb = self.root / "gdb"
        with (
            mock.patch.object(bl.os.path, "isfile", return_value=False),
            mock.patch.object(
                bl.shutil,
                "which",
                lambda name: str(path_gdb) if name == "gdb" else None,
            ),
        ):
            self.assertEqual(bl.gdb_candidates(), [str(path_gdb)])

    def test_capture_stacks_falls_back_to_next_candidate(self):
        failing = self.script(
            "failing-gdb.py",
            "import sys\nsys.stderr.write('cannot attach\\n')\nsys.exit(1)\n",
        )
        working = self.script(
            "working-gdb.py",
            "import sys\nsys.stdout.write('Thread 1 (Thread 0x0 (LWP 42)):\\n#0  sleep ()\\n')\n",
        )
        logf = io.StringIO()
        with (
            mock.patch.object(bl, "gdb_candidates", lambda: [str(failing), str(working)]),
            mock.patch.object(bl, "_rust_pretty_printers", lambda: ([], {})),
        ):
            result = bl.capture_stacks(os.getpid(), self.diag, logf, True)

        self.assertTrue(result["ok"])
        self.assertEqual(result["tool"], str(working))
        self.assertEqual(len(result["attempts"]), 2)
        stacks = (self.diag / "stacks.txt").read_text()
        self.assertIn("Thread 1", stacks)
        self.assertIn("#0", stacks)

    def test_capture_stacks_gdb_positive(self):
        # Real attach against a live child. Skips where gdb is absent or lacks
        # ptrace permission so the suite stays green on unprivileged CI.
        if not bl.gdb_candidates():
            self.skipTest("no gdb binary available")
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        self.addCleanup(child.wait)
        self.addCleanup(child.terminate)
        logf = io.StringIO()
        result = bl.capture_stacks(child.pid, self.diag, logf, True)
        if not result["ok"]:
            self.skipTest(f"gdb cannot attach here: {result['reason']}")

        self.assertGreaterEqual(int((self.diag / "threads.txt").read_text().strip()), 1)
        stacks = (self.diag / "stacks.txt").read_text()
        self.assertIn("Thread ", stacks)
        self.assertIn("#0", stacks)


class ImageHomeAndManifestTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_image_home_src_not_directory_raises(self):
        out = self.root / "cell"
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(OUT=str(out), IMAGE_HOME_SRC=str(self.root / "nope")))
        f = self.root / "f.txt"
        f.write_text("x")
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(OUT=str(out), IMAGE_HOME_SRC=str(f)))

    def test_image_home_src_override_no_images_dir_and_env(self):
        src = self.root / "src"
        src.mkdir()
        (src / "a.jpg").write_bytes(b"x")
        (src / "b.jpg").write_bytes(b"x")
        out = self.root / "cell"
        env = base_env(OUT=str(out), IMAGE_HOME_SRC=str(src), DATASET_ID="ds-1")
        cfg = bl.config_from_env(env)
        self.assertEqual(cfg["image_home"], src)
        bl.prepare_out(cfg)
        self.assertFalse((out / "images").exists())
        self.assertTrue(src.exists())
        self.assertEqual(sorted(p.name for p in src.iterdir()), ["a.jpg", "b.jpg"])
        se = bl.server_env(cfg, env)
        self.assertEqual(se["PICASU_IMAGE_HOME"], str(src))
        bl.write_manifest(cfg, env)
        m = json.loads((out / "run-manifest.json").read_text())
        self.assertEqual(m["dataset"]["count"], 2)
        self.assertEqual(m["dataset"]["id"], "ds-1")
        self.assertEqual(m["dataset"]["source"], str(src))
        self.assertIn("cpu_model", m)
        self.assertIn("mem_total", m)
        self.assertIn("fs_type", m)
        self.assertIn("binary_sha256", m)
        self.assertIn("PICASU_OBSERVER_STATS", m)

    def test_observer_flag_recording_on_off(self):
        for val, expected in ((None, "unset"), ("1", "1"), ("0", "0")):
            out = self.root / f"cell-{val}"
            env = base_env(OUT=str(out))
            if val is not None:
                env["PICASU_OBSERVER_STATS"] = val
            cfg = bl.config_from_env(env)
            bl.prepare_out(cfg)
            bl.write_manifest(cfg, env)
            m = json.loads((cfg["out"] / "run-manifest.json").read_text())
            self.assertEqual(m["PICASU_OBSERVER_STATS"], expected)
            self.assertEqual(m["observer_stats"], expected)

    def test_local_dataset_recorded_as_out_local(self):
        out = self.root / "cell"
        (out / "data").mkdir(parents=True)
        (out / "images").mkdir(parents=True)
        (out / "images" / "x.jpg").write_bytes(b"x")
        env = base_env(OUT=str(out), FRESH="0", ADMIN_PW="pw")
        cfg = bl.config_from_env(env)
        bl.prepare_out(cfg)
        bl.write_manifest(cfg, env)
        m = json.loads((out / "run-manifest.json").read_text())
        self.assertEqual(m["dataset"]["source"], "out-local")
        self.assertEqual(m["dataset"]["count"], 1)
        self.assertEqual(m["dataset"]["id"], "out-local")


if __name__ == "__main__":
    unittest.main()
