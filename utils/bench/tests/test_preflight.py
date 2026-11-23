#!/usr/bin/env python3
"""Tests for preflight.py. Offline unit tests: stdlib only, no sockets,
no subprocesses, no external binaries.

Covers every check's ok/fail path against tmp fixtures (a listening
socket would be needed for the busy-port FAIL — that path is covered
by live batteries instead), the checklist output shape, exit codes,
and that the launcher runs the checklist before it spawns the server.
"""

import contextlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))


def load_module(name: str, filename: str):
    """Import one bench module by name."""
    module = sys.modules.get(name)
    if module is not None:
        return module
    module = __import__(name)
    sys.modules[name] = module
    return module


bl = load_module("backend_launcher", "backend_launcher.py")
pf = load_module("preflight", "preflight.py")


def base_env(**kw):
    env = {
        "PICASU_BIN": sys.executable,
        "PORT": "59999",
    }
    env.update(kw)
    return env


def capture(argv=None):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        rc = pf.main(argv)
    return rc, out.getvalue(), err.getvalue()


class CheckShapeTest(unittest.TestCase):
    def test_ok_line(self):
        check = pf.Check("port", True, "5713 free")

        self.assertTrue(check.line().startswith("ok port:"))
        self.assertIn("5713 free", check.line())

    def test_fail_line_carries_hint(self):
        check = pf.Check("sandbox", False, "sandbox manifest missing", "run: just bench prepare")

        self.assertTrue(check.line().startswith("FAIL sandbox:"))
        self.assertIn("sandbox manifest missing", check.line())
        self.assertIn("run: just bench prepare", check.line())
        self.assertIn("run: just bench prepare", check.message())


class ServerBinaryCheckTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_explicit_executable_ok(self):
        check = pf.check_server_binary({"PICASU_BIN": sys.executable})
        self.assertTrue(check.ok, check.line())
        self.assertEqual(check.detail, sys.executable)

    def test_explicit_missing_fails_with_build_hint(self):
        check = pf.check_server_binary({"PICASU_BIN": "/nonexistent/picasu"})
        self.assertFalse(check.ok)
        self.assertIn("missing executable server binary", check.detail)
        self.assertIn("cargo build --profile release --bin picasu", check.hint)

    def test_default_binary_resolves_under_target_profile(self):
        fake_repo = self.root / "repo"
        binary = fake_repo / "target" / "debug" / "picasu"
        binary.parent.mkdir(parents=True)
        binary.write_text("#!/bin/sh\n")
        binary.chmod(0o755)
        with mock.patch.object(bl, "REPO_ROOT", fake_repo):
            check = pf.check_server_binary({"BUILD_PROFILE": "debug"})
        self.assertTrue(check.ok, check.line())
        self.assertEqual(check.detail, str(binary))


class ArchiveCheckTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_missing_directory_fails(self):
        check = pf.check_archive(self.root / "nope")
        self.assertFalse(check.ok)
        self.assertIn("archive directory not found", check.detail)

    def test_directory_without_jpg_fails(self):
        archive = self.root / "set-100"
        archive.mkdir()
        (archive / "notes.txt").write_text("x")
        check = pf.check_archive(archive)
        self.assertFalse(check.ok)
        self.assertIn("holds no .jpg images", check.detail)

    def test_directory_with_jpg_ok(self):
        archive = self.root / "set-100"
        archive.mkdir()
        for name in ("a.jpg", "b.jpg"):
            (archive / name).write_bytes(b"x")
        check = pf.check_archive(archive)
        self.assertTrue(check.ok, check.line())
        self.assertIn("2 jpg", check.detail)


class SandboxCheckTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_missing_manifest_fails_with_prepare_hint(self):
        sandbox = self.root / "sandbox"
        sandbox.mkdir()
        check = pf.check_sandbox_manifest(sandbox)
        self.assertFalse(check.ok)
        self.assertIn("sandbox-manifest.json", check.detail)
        self.assertIn("bench prepare", check.detail)

    def test_not_a_directory_fails(self):
        target = self.root / "file"
        target.write_text("x")
        check = pf.check_sandbox_manifest(target)
        self.assertFalse(check.ok)
        self.assertIn("must be a directory", check.detail)

    def test_manifest_ok_reports_count(self):
        sandbox = self.root / "sandbox"
        sandbox.mkdir()
        (sandbox / "a.jpg").write_bytes(b"x")
        (sandbox / "sandbox-manifest.json").write_text(json.dumps({"count": 3}))
        check = pf.check_sandbox_manifest(sandbox)
        self.assertTrue(check.ok, check.line())
        self.assertIn("count=3", check.detail)


class PortCheckTest(unittest.TestCase):
    def test_free_port_ok(self):
        check = pf.check_port(59999)
        self.assertTrue(check.ok, check.line())
        self.assertIn("free", check.detail)

    def test_invalid_port_fails_with_launcher_message(self):
        for bad in ("abc", "0", "70000"):
            check = pf.check_port(bad)
            self.assertFalse(check.ok, bad)
            self.assertIn("PORT must be", check.detail)


class OutCheckTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_fresh_under_temp_root_ok(self):
        check = pf.check_out(self.root / "cell", fresh=True)
        self.assertTrue(check.ok, check.line())
        self.assertIn("will be created", check.detail)

    def test_existing_directory_ok(self):
        out = self.root / "cell"
        out.mkdir()
        check = pf.check_out(out, fresh=False)
        self.assertTrue(check.ok, check.line())
        self.assertIn("writable", check.detail)

    def test_fresh_outside_temp_root_fails(self):
        check = pf.check_out("/picasu-preflight-fixture/out", fresh=True)
        self.assertFalse(check.ok)
        self.assertIn("refusing to wipe outside a temp root", check.detail)

    def test_repo_out_fails(self):
        check = pf.check_out(bl.REPO_ROOT / "cell", fresh=False)
        self.assertFalse(check.ok)
        self.assertIn("must not be inside the repo", check.detail)

    def test_root_out_fails(self):
        check = pf.check_out("/", fresh=False)
        self.assertFalse(check.ok)
        self.assertIn("refusing to use OUT='/'", check.detail)

    @unittest.skipIf(os.geteuid() == 0, "root bypasses directory permission bits")
    def test_unwritable_parent_fails(self):
        parent = self.root / "readonly"
        parent.mkdir()
        parent.chmod(0o555)
        self.addCleanup(parent.chmod, 0o755)
        check = pf.check_out(parent / "cell", fresh=False)
        self.assertFalse(check.ok)
        self.assertIn("not writable", check.detail)


class PreflightAssemblyTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def names(self, checks):
        return [check.name for check in checks]

    def test_default_checklist(self):
        checks = pf.preflight(base_env())
        self.assertEqual(self.names(checks), ["binary", "port"])
        self.assertTrue(all(check.ok for check in checks), [c.line() for c in checks])

    def test_archive_and_out_and_sandbox_included_only_when_requested(self):
        archive = self.root / "set-100"
        archive.mkdir()
        (archive / "a.jpg").write_bytes(b"x")
        sandbox = self.root / "sandbox"
        sandbox.mkdir()
        (sandbox / "sandbox-manifest.json").write_text(json.dumps({"count": 1}))
        checks = pf.preflight(
            base_env(IMAGE_HOME_SRC=str(sandbox)),
            out=self.root / "cell",
            archive=archive,
        )
        self.assertEqual(self.names(checks), ["binary", "archive", "sandbox", "out", "port"])
        self.assertTrue(all(check.ok for check in checks), [c.line() for c in checks])

    def test_failed_checks_are_reported_not_raised(self):
        checks = pf.preflight(
            base_env(PICASU_BIN="/nonexistent/picasu"),
            out="/picasu-preflight-fixture/out",
            archive=self.root / "nope",
        )
        failed = [check for check in checks if not check.ok]
        self.assertEqual([check.name for check in failed], ["binary", "archive", "out"])


class ChecklistOutputTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_main_ok_exit_zero(self):
        env = base_env(OUT=str(self.root / "cell"))
        with mock.patch.dict(os.environ, env, clear=True):
            rc, out, err = capture([])
        self.assertEqual(rc, 0)
        self.assertEqual(err, "")
        lines = out.splitlines()
        self.assertTrue(lines)
        self.assertTrue(all(line.startswith("ok ") for line in lines), lines)
        self.assertEqual(
            [line.split(":")[0] for line in lines],
            ["ok binary", "ok out", "ok port"],
        )

    def test_main_fail_exit_one(self):
        env = base_env(
            PICASU_BIN="/nonexistent/picasu",
            OUT="/picasu-preflight-fixture/out",
        )
        with mock.patch.dict(os.environ, env, clear=True):
            rc, out, err = capture([])
        self.assertEqual(rc, 1)
        self.assertNotIn("FAIL", out)
        failed = [line.split(":")[0] for line in err.splitlines()]
        self.assertEqual(failed, ["FAIL binary", "FAIL out"])

    def test_main_archive_flag(self):
        with mock.patch.dict(os.environ, base_env(), clear=True):
            rc, out, err = capture(["--archive", str(self.root / "nope")])
        self.assertEqual(rc, 1)
        self.assertIn("FAIL archive:", err)


class CallerReuseTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_launcher_run_backend_runs_the_checklist_before_spawning(self):
        out = self.root / "cell"
        env = base_env(OUT=str(out))
        cfg = bl.config_from_env(env)
        spawned = []

        def spawn(cfg_, server_env_):
            spawned.append(True)
            raise AssertionError("spawn must not run when the preflight fails")

        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            mock.patch.object(bl, "port_in_use", return_value=True),
            contextlib.redirect_stdout(stdout),
            contextlib.redirect_stderr(stderr),
        ):
            rc = bl.run_backend(cfg, env, spawn=spawn)
        self.assertEqual(rc, 1)
        self.assertEqual(spawned, [])
        self.assertFalse(out.exists())
        self.assertIn("FAIL port:", stderr.getvalue())
        self.assertIn("ok binary:", stdout.getvalue())


if __name__ == "__main__":
    unittest.main()
