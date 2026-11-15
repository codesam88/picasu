#!/usr/bin/env python3
"""Offline unit tests for the workload wrapper. Stdlib only, no sockets,
no subprocesses, no external binaries.

Live behavior (cells against real or stub servers) is verified by
running batteries, not by this suite.
"""

import contextlib
import http.cookiejar
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

BENCH = Path(__file__).resolve().parent.parent


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


def load_workload():
    spec = importlib.util.spec_from_file_location("workload", BENCH / "workload.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class ImageSetTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.tmp_path = Path(self.tmp.name)
        self.workload = load_workload()

    def _run_prepare(self, files, images, image_path=None, repo=None):
        repo = repo or (self.tmp_path / "repo")
        repo.mkdir(exist_ok=True)
        for name in files:
            (repo / name).write_bytes(b"x" * 1000)
        out = self.tmp_path / "cell"
        out.mkdir(exist_ok=True)
        cfg = self.workload.config.WorkloadConfig(
            out=out,
            image_repo=repo,
            image_path=image_path or out / "images",
            images=images,
        )
        logger = self.workload.Logger(out / "prepare.log")
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                return self.workload.prepare_images(cfg, logger)
        finally:
            logger.close()

    def test_seeded_copy_takes_sorted_prefix(self):
        count, size = self._run_prepare(
            [
                "img_0003.jpg",
                "img_0001.jpg",
                "img_0005.jpg",
                "img_0002.jpg",
                "img_0004.jpg",
                "img_0006.jpg",
            ],
            4,
        )
        self.assertEqual(count, 4)
        self.assertEqual(size, 4000)
        got = sorted(p.name for p in (self.tmp_path / "cell" / "images").glob("*.jpg"))
        self.assertEqual(got, ["img_0001.jpg", "img_0002.jpg", "img_0003.jpg", "img_0004.jpg"])
        log = (self.tmp_path / "cell" / "prepare.log").read_text()
        self.assertIn("generated=4 bytes=4000", log)
        self.assertIn("mode=linked(", log)

    def test_short_repo_fails_with_counts(self):
        with self.assertRaises(self.workload.ConfigError) as ctx:
            self._run_prepare(["img_0001.jpg", "img_0002.jpg"], 4)
        self.assertIn("holds 2 images", str(ctx.exception))
        self.assertIn("IMAGES=4", str(ctx.exception))

    def test_missing_image_repo_fails(self):
        with self.assertRaises(self.workload.ConfigError) as ctx:
            self.workload.config_from_env({"IMAGES": "4"})
        self.assertIn("IMAGE_REPO is required", str(ctx.exception))

    def test_image_path_override(self):
        custom = self.tmp_path / "custom-images"
        self._run_prepare(["img_0001.jpg", "img_0002.jpg"], 2, image_path=custom)
        self.assertEqual(len(list(custom.glob("*.jpg"))), 2)
        self.assertFalse((self.tmp_path / "cell" / "images").exists())


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
                "IMAGES": "7",
                "IMAGE_REPO": str(tmp_path),
                "SCAN_REPEATS": "2",
                "REPEATS": "4",
                "WARMUP": "1",
                "RUNS_DURING": "2",
                "FETCHES": "3",
                "SCAN_TIMEOUT_S": "0",
            }
            cfg = workload.config_from_env(env)
            self.assertEqual(cfg.base_url, "http://127.0.0.1:9999")
            self.assertEqual(cfg.out, cell.resolve())
            self.assertEqual(cfg.images, 7)
            self.assertEqual(cfg.scan_repeats, 2)
            self.assertEqual(cfg.repeats, 4)
            self.assertEqual(cfg.warmup, 1)
            self.assertEqual(cfg.runs_during, 2)
            self.assertEqual(cfg.fetches, 3)

    def test_workload_defaults_and_validation(self):
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
            repo = tmp_path / "repo"
            repo.mkdir()
            base = {"LAUNCHER_INFO": str(info), "IMAGE_REPO": str(repo)}
            cfg = workload.config_from_env(dict(base))
            self.assertEqual(cfg.workload, "control")
            self.assertEqual(cfg.storm_clients, 8)
            self.assertEqual(cfg.storm_ops, 10)
            self.assertEqual(cfg.storm_margin_s, 0.5)
            for bad in ("storm-everything", "CONTROL", ""):
                with self.subTest(workload=bad):
                    with self.assertRaises(workload.ConfigError):
                        workload.config_from_env(dict(base, WORKLOAD=bad))

    def test_probe_and_upload_env_defaults_and_overrides(self):
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
            repo = tmp_path / "repo"
            repo.mkdir()
            base = {"LAUNCHER_INFO": str(info), "IMAGE_REPO": str(repo)}

            cfg = workload.config_from_env(dict(base))
            self.assertEqual(cfg.probe_page, 1000)
            self.assertEqual(cfg.index_settle_timeout_s, 600.0)
            self.assertEqual(cfg.upload_timeout_s, 30.0)
            self.assertEqual(cfg.upload_post_timeout_s, 60.0)

            cfg = workload.config_from_env(
                dict(
                    base,
                    PROBE_PAGE="250",
                    INDEX_SETTLE_TIMEOUT_S="30",
                    UPLOAD_TIMEOUT_S="45",
                    UPLOAD_POST_TIMEOUT_S="12.5",
                )
            )
            self.assertEqual(cfg.probe_page, 250)
            self.assertEqual(cfg.index_settle_timeout_s, 30.0)
            self.assertEqual(cfg.upload_timeout_s, 45.0)
            self.assertEqual(cfg.upload_post_timeout_s, 12.5)

            for name, value in (
                ("PROBE_PAGE", "0"),
                ("PROBE_PAGE", "page"),
                ("INDEX_SETTLE_TIMEOUT_S", "0"),
                ("UPLOAD_TIMEOUT_S", "0"),
                ("UPLOAD_POST_TIMEOUT_S", "0"),
            ):
                with self.subTest(name=name, value=value):
                    with self.assertRaises(workload.ConfigError):
                        workload.config_from_env(dict(base, **{name: value}))

    def test_missing_hyperfine_fails_fast(self):
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
            repo = tmp_path / "repo"
            repo.mkdir()
            env = {
                "LAUNCHER_INFO": str(info),
                "IMAGE_REPO": str(repo),
            }
            with mock.patch.object(workload.shutil, "which", return_value=None):
                with self.assertRaises(workload.ConfigError) as ctx:
                    workload.config_from_env(env)
                self.assertIn(
                    "hyperfine is required (run: just install hyperfine)", str(ctx.exception)
                )


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


class RecordingUploadClient:
    def __init__(self, names):
        self.names = list(names)
        self.timeouts = []

    def upload(self, path, file_path, timeout=60.0):
        self.timeouts.append(timeout)
        name = self.names[len(self.timeouts) - 1]
        return 200, json.dumps({"files": [name]}).encode(), 0.01


class UploadStimulusTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.cell = root / "cell"
        self.cell.mkdir()
        self.repo = root / "repo"
        self.repo.mkdir()
        for index in range(1, 4):
            (self.repo / f"img_{index:04d}.jpg").write_bytes(b"x" * 100)
        self.image_path = root / "images"
        self.image_path.mkdir()
        self.workload = load_workload()

    def _logger(self):
        logger = self.workload.Logger(self.cell / "harness-a.log")
        self.addCleanup(logger.close)
        return logger

    def _cfg(self, **overrides):
        fields = {
            "out": self.cell,
            "image_repo": self.repo,
            "image_path": self.image_path,
            "upload_files": 2,
            "upload_timeout_s": 0.1,
            "poll_interval_s": 0.01,
            "upload_post_timeout_s": 60.0,
        }
        fields.update(overrides)
        return self.workload.config.WorkloadConfig(**fields)

    def test_post_budget_reaches_the_upload_call(self):
        client = RecordingUploadClient(["u1.jpg", "u2.jpg"])
        with (
            mock.patch.object(self.workload, "_uploaded_row_visible", return_value=True),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            self.workload.run_upload_stimulus(
                self._cfg(upload_post_timeout_s=0.25), client, self._logger()
            )
        self.assertEqual(client.timeouts, [0.25, 0.25])
        log = (self.cell / "harness-a.log").read_text()
        self.assertRegex(log, r"upload file=u1\.jpg submit_ms=\d+ visible_ms=\d+")
        self.assertRegex(log, r"upload file=u2\.jpg submit_ms=\d+ visible_ms=\d+")
        self.assertRegex(log, r"upload submit_p50=\d+ visible_p50=\d+")


if __name__ == "__main__":
    unittest.main()
