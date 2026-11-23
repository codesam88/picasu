#!/usr/bin/env python3
"""Offline unit tests for the workload wrapper. Stdlib only, no sockets,
no subprocesses, no external binaries.

Live behavior (cells against real or stub servers) is verified by
running batteries, not by this suite.
"""

import contextlib
import http.cookiejar
import io
import json
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))


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
    import workload

    return workload


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


class UploadSourceSelectionTest(unittest.TestCase):
    """The upload stimulus never re-uploads an image the cell already staged."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.workload = load_workload()
        self.repo = self.root / "repo"
        self.repo.mkdir()
        for index in range(1, 6):
            (self.repo / f"img_{index:04d}.jpg").write_bytes(b"x")
        self.staged = self.root / "images"
        self.staged.mkdir()
        for index in range(1, 4):
            (self.staged / f"img_{index:04d}.jpg").write_bytes(b"x")

    def _cfg(self, **overrides):
        fields = {
            "out": self.root / "cell",
            "image_repo": self.repo,
            "image_path": self.staged,
            "upload_files": 2,
        }
        fields.update(overrides)
        return self.workload.config.WorkloadConfig(**fields)

    def test_same_repo_uses_files_beyond_the_staged_prefix(self):
        names = [path.name for path in self.workload._select_upload_sources(self._cfg())]
        self.assertEqual(names, ["img_0004.jpg", "img_0005.jpg"])

    def test_same_repo_without_margin_fails(self):
        with self.assertRaises(self.workload.WorkloadError):
            self.workload._select_upload_sources(self._cfg(upload_files=3))

    def test_distinct_upload_repo_is_its_own_set(self):
        upload = self.root / "upload"
        upload.mkdir()
        for index in range(1, 4):
            (upload / f"img_{index:04d}.jpg").write_bytes(b"y")
        names = [
            path.name
            for path in self.workload._select_upload_sources(
                self._cfg(upload_repo=upload, upload_files=2)
            )
        ]
        self.assertEqual(names, ["img_0001.jpg", "img_0002.jpg"])

    def test_too_few_upload_files_fails(self):
        upload = self.root / "upload"
        upload.mkdir()
        (upload / "only.jpg").write_bytes(b"y")
        with self.assertRaises(self.workload.WorkloadError):
            self.workload._select_upload_sources(self._cfg(upload_repo=upload, upload_files=2))


if __name__ == "__main__":
    unittest.main()


class ScrollProbeTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.cell = self.root / "cell"
        self.cell.mkdir()
        self.workload = load_workload()

    def _cfg(self, **overrides):
        fields = {"out": self.cell, "images": 5, "upload_files": 1, "probe_page": 1000}
        fields.update(overrides)
        return self.workload.config.WorkloadConfig(**fields)

    def _client(self):
        calls = []

        class Client:
            def get(self, path, bearer=None, timeout=30.0):
                calls.append((path, bearer, timeout))
                return 200, b"[]", 0.012

        client = Client()
        client.calls = calls
        return client

    def test_scroll_probe_issues_expected_requests(self):
        cfg = self._cfg()
        client = self._client()
        result = self.workload.scroll_probe(cfg, client, 42, "tok", total_rows=100, positions=3)
        self.assertIn("scrollbar", result)
        self.assertEqual(len(result["scrollbar"]), 1)
        self.assertGreater(len(result["rows"]), 0)
        self.assertGreater(len(result["data"]), 0)
        paths = [call[0] for call in client.calls]
        self.assertTrue(any("/get/get-scroll-bar?timestamp=42" in p for p in paths))
        self.assertTrue(any("/get/get-rows?index=" in p and "timestamp=42" in p for p in paths))
        self.assertTrue(any("/get/get-data?timestamp=42" in p for p in paths))
        # bearer token forwarded
        self.assertTrue(all(call[1] == "tok" for call in client.calls))

    def test_scroll_probe_writes_artifact(self):
        cfg = self._cfg()
        client = self._client()
        self.workload.scroll_probe(
            cfg,
            client,
            1,
            "t",
            total_rows=10,
            out=self.cell,
            round_no=1,
            round_dir=self.cell / "rounds",
        )
        self.assertTrue((self.cell / "scroll-probe.tsv").is_file())


class ScanPhaseTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()

    def _write(self, doc):
        (self.out / "status-last.json").write_text(json.dumps(doc))

    def test_valid(self):
        self._write({"state": "running", "scanned": 1, "processed": 2})
        guard = self.workload.ScanPhase(self.out, "probe")
        guard.start()
        self._write({"state": "running", "scanned": 2, "processed": 3})
        result = guard.end()
        self.assertTrue(result["valid"])
        self.assertEqual(
            set(result.keys()),
            {
                "valid",
                "reason",
                "state_start",
                "state_end",
                "scanned_start",
                "scanned_end",
                "processed_start",
                "processed_end",
                "wall_ms",
                "progress",
            },
        )
        self.assertEqual(result["state_start"], "running")
        self.assertEqual(result["state_end"], "running")

    def test_invalid_start_terminal(self):
        self._write({"state": "completed", "scanned": 1, "processed": 2})
        guard = self.workload.ScanPhase(self.out, "probe")
        guard.start()
        self._write({"state": "completed", "scanned": 2, "processed": 3})
        result = guard.end()
        self.assertFalse(result["valid"])
        self.assertIn("terminal state at start", result["reason"])

    def test_invalid_end_terminal(self):
        self._write({"state": "running", "scanned": 1, "processed": 2})
        guard = self.workload.ScanPhase(self.out, "probe")
        guard.start()
        self._write({"state": "completed", "scanned": 2, "processed": 3})
        result = guard.end()
        self.assertFalse(result["valid"])
        self.assertIn("terminal state at end", result["reason"])

    def test_missing_snapshot(self):
        (self.out / "status-last.json").write_text(
            json.dumps({"state": "running", "scanned": 1, "processed": 1})
        )
        g2 = self.workload.ScanPhase(self.out, "probe")
        g2.start()
        (self.out / "status-last.json").unlink()
        res = g2.end()
        self.assertFalse(res["valid"])
        self.assertIn("missing", res["reason"])

    def test_missing_counter(self):
        self._write({"state": "running", "processed": 2})
        guard = self.workload.ScanPhase(self.out, "probe")
        guard.start()
        self._write({"state": "running", "processed": 3})
        result = guard.end()
        self.assertFalse(result["valid"])
        self.assertIn("counter", result["reason"])

    def test_non_numeric_counter(self):
        self._write({"state": "running", "scanned": "x", "processed": 2})
        guard = self.workload.ScanPhase(self.out, "probe")
        guard.start()
        self._write({"state": "running", "scanned": "y", "processed": 3})
        result = guard.end()
        self.assertFalse(result["valid"])
        self.assertIn("non-numeric", result["reason"])

    def test_no_progress(self):
        self._write({"state": "running", "scanned": 1, "processed": 2})
        guard = self.workload.ScanPhase(self.out, "probe")
        guard.start()
        self._write({"state": "running", "scanned": 1, "processed": 2})
        result = guard.end()
        self.assertTrue(result["valid"])
        self.assertFalse(result["progress"])

    def test_injected_snapshot(self):
        snaps = iter(
            [
                {"state": "running", "scanned": 1, "processed": 2, "ts": 1.0},
                {"state": "running", "scanned": 2, "processed": 3, "ts": 2.0},
            ]
        )
        guard = self.workload.ScanPhase(self.out, "probe", snapshot=lambda: next(snaps))
        guard.start()
        result = guard.end()
        self.assertTrue(result["valid"])
        self.assertEqual(result["wall_ms"], 1000)


class ProbeLoopTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None
        self._orig_probe_phase = self.workload.probe_phase
        self._orig_build = self.workload.build_probe_list
        self._orig_probe_round = self.workload.probe_round
        self._orig_snap = self.workload._status_snapshot
        self._orig_state = self.workload._status_state

    def tearDown(self):
        self.workload.time.sleep = self._orig_sleep
        self.workload.probe_phase = self._orig_probe_phase
        self.workload.build_probe_list = self._orig_build
        self.workload.probe_round = self._orig_probe_round
        self.workload._status_snapshot = self._orig_snap
        self.workload._status_state = self._orig_state

    def _cfg(self, **overrides):
        fields = {
            "out": self.out,
            "probe_interval_s": 0.0,
            "probe_refresh_s": 0.0,
            "scan_timeout_s": 0.0,
            "min_valid_rounds": 1,
        }
        fields.update(overrides)
        return self.workload.config.WorkloadConfig(**fields)

    def test_continues_after_rows_reach_expected(self):
        cfg = self._cfg()
        self.workload.build_probe_list = lambda cfg, client: ["a\tt"]
        self.workload.probe_round = lambda *a, **k: True
        counter = {"n": 0}

        def snap(out):
            counter["n"] += 1
            state = "running" if counter["n"] <= 12 else "completed"
            return {
                "state": state,
                "scanned": counter["n"],
                "processed": counter["n"],
                "ts": float(counter["n"]),
            }

        def st(out):
            return "running" if counter["n"] < 12 else "completed"

        self.workload._status_snapshot = snap
        self.workload._status_state = st
        logger = self.workload.Logger(self.out / "h.log")
        self.addCleanup(logger.close)
        result = self.workload.probe_phase(cfg, object(), logger, self.out / "rounds-run-1")
        self.assertGreaterEqual(result["rounds_total"], 3)
        self.assertEqual(result["rounds_valid"], result["rounds_total"])

    def test_probes_with_unchanged_rows(self):
        cfg = self._cfg()
        calls = {"n": 0}

        def build(cfg, client):
            calls["n"] += 1
            return ["a\tt"]

        self.workload.build_probe_list = build
        self.workload.probe_round = lambda *a, **k: True
        counter = {"n": 0}

        def snap(out):
            counter["n"] += 1
            state = "running" if counter["n"] <= 8 else "completed"
            return {
                "state": state,
                "scanned": counter["n"],
                "processed": counter["n"],
                "ts": float(counter["n"]),
            }

        def st(out):
            return "running" if counter["n"] < 8 else "completed"

        self.workload._status_snapshot = snap
        self.workload._status_state = st
        logger = self.workload.Logger(self.out / "h.log")
        self.addCleanup(logger.close)
        result = self.workload.probe_phase(cfg, object(), logger, self.out / "rounds-run-1")
        self.assertGreaterEqual(result["rounds_total"], 2)

    def test_failed_round_excluded(self):
        cfg = self._cfg()
        self.workload.build_probe_list = lambda cfg, client: ["a\tt"]

        def fake_probe(*a, **k):
            round_no = a[4]
            return round_no != 2

        self.workload.probe_round = fake_probe
        counter = {"n": 0}

        def snap(out):
            counter["n"] += 1
            state = "running" if counter["n"] <= 12 else "completed"
            return {
                "state": state,
                "scanned": counter["n"],
                "processed": counter["n"],
                "ts": float(counter["n"]),
            }

        def st(out):
            return "running" if counter["n"] < 12 else "completed"

        self.workload._status_snapshot = snap
        self.workload._status_state = st
        logger = self.workload.Logger(self.out / "h.log")
        self.addCleanup(logger.close)
        result = self.workload.probe_phase(cfg, object(), logger, self.out / "rounds-run-1")
        self.assertEqual(result["rounds_valid"], result["rounds_total"] - 1)

    def test_aborts_below_min_valid_rounds(self):
        import json as _json

        class FakeClient:
            def post(self, path, payload=None):
                return 202, b"{}", 0.0

            def __init__(self):
                self.calls = 0

            def get(self, path):
                self.calls += 1
                if self.calls == 1:
                    body = _json.dumps(
                        {
                            "state": "running",
                            "scanned": 1,
                            "matched": 1,
                            "processed": 1,
                            "failed": 0,
                        }
                    ).encode()
                else:
                    body = _json.dumps(
                        {
                            "state": "completed",
                            "scanned": 1,
                            "matched": 1,
                            "processed": 1,
                            "failed": 0,
                        }
                    ).encode()
                return 200, body, 0.0

        cfg = self._cfg(min_valid_rounds=3)
        self.workload.probe_phase = lambda *a, **k: {
            "rounds_total": 1,
            "rounds_valid": 0,
            "invalid_reasons": ["bad"],
            "progress": False,
        }
        with self.assertRaises(self.workload.WorkloadError):
            self.workload.run_scans(cfg, FakeClient(), self.workload.Logger(self.out / "h2.log"))


class ProbeRefreshLoopTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None
        self._orig_probe_phase = self.workload.probe_phase
        self._orig_build = self.workload.build_probe_list
        self._orig_probe_round = self.workload.probe_round
        self._orig_snap = self.workload._status_snapshot
        self._orig_state = self.workload._status_state

    def tearDown(self):
        self.workload.time.sleep = self._orig_sleep
        self.workload.probe_phase = self._orig_probe_phase
        self.workload.build_probe_list = self._orig_build
        self.workload.probe_round = self._orig_probe_round
        self.workload._status_snapshot = self._orig_snap
        self.workload._status_state = self._orig_state

    def _cfg(self, **overrides):
        fields = {
            "out": self.out,
            "probe_interval_s": 0.0,
            "probe_refresh_s": 10.0,
            "scan_timeout_s": 0.0,
            "min_valid_rounds": 1,
        }
        fields.update(overrides)
        return self.workload.config.WorkloadConfig(**fields)

    def test_build_probe_list_cached(self):
        cfg = self._cfg()
        calls = {"n": 0}

        def build(cfg, client):
            calls["n"] += 1
            return ["a\tt"]

        self.workload.build_probe_list = build
        self.workload.probe_round = lambda *a, **k: True
        counter = {"n": 0}

        def snap(out):
            counter["n"] += 1
            state = "running" if counter["n"] <= 12 else "completed"
            return {
                "state": state,
                "scanned": counter["n"],
                "processed": counter["n"],
                "ts": float(counter["n"]),
            }

        def st(out):
            return "running" if counter["n"] < 12 else "completed"

        self.workload._status_snapshot = snap
        self.workload._status_state = st
        logger = self.workload.Logger(self.out / "h.log")
        self.addCleanup(logger.close)
        result = self.workload.probe_phase(cfg, object(), logger, self.out / "rounds-run-1")
        self.assertGreaterEqual(result["rounds_total"], 2)
        self.assertEqual(calls["n"], 1)


class ProbeRefreshZeroLoopTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None
        self._orig_probe_phase = self.workload.probe_phase
        self._orig_build = self.workload.build_probe_list
        self._orig_probe_round = self.workload.probe_round
        self._orig_snap = self.workload._status_snapshot
        self._orig_state = self.workload._status_state

    def tearDown(self):
        self.workload.time.sleep = self._orig_sleep
        self.workload.probe_phase = self._orig_probe_phase
        self.workload.build_probe_list = self._orig_build
        self.workload.probe_round = self._orig_probe_round
        self.workload._status_snapshot = self._orig_snap
        self.workload._status_state = self._orig_state

    def _cfg(self, **overrides):
        fields = {
            "out": self.out,
            "probe_interval_s": 0.0,
            "probe_refresh_s": 0.0,
            "scan_timeout_s": 0.0,
            "min_valid_rounds": 1,
        }
        fields.update(overrides)
        return self.workload.config.WorkloadConfig(**fields)

    def test_build_probe_list_once_when_refresh_zero(self):
        cfg = self._cfg()
        calls = {"n": 0}

        def build(cfg, client):
            calls["n"] += 1
            return ["a\tt"]

        self.workload.build_probe_list = build
        self.workload.probe_round = lambda *a, **k: True
        counter = {"n": 0}

        def snap(out):
            counter["n"] += 1
            state = "running" if counter["n"] <= 12 else "completed"
            return {
                "state": state,
                "scanned": counter["n"],
                "processed": counter["n"],
                "ts": float(counter["n"]),
            }

        def st(out):
            return "running" if counter["n"] < 12 else "completed"

        self.workload._status_snapshot = snap
        self.workload._status_state = st
        logger = self.workload.Logger(self.out / "h.log")
        self.addCleanup(logger.close)
        result = self.workload.probe_phase(cfg, object(), logger, self.out / "rounds-run-1")
        self.assertGreaterEqual(result["rounds_total"], 2)
        self.assertEqual(calls["n"], 1)


class ProbeNoProgressLoopTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None
        self._orig_probe_phase = self.workload.probe_phase
        self._orig_build = self.workload.build_probe_list
        self._orig_probe_round = self.workload.probe_round
        self._orig_snap = self.workload._status_snapshot
        self._orig_state = self.workload._status_state

    def tearDown(self):
        self.workload.time.sleep = self._orig_sleep
        self.workload.probe_phase = self._orig_probe_phase
        self.workload.build_probe_list = self._orig_build
        self.workload.probe_round = self._orig_probe_round
        self.workload._status_snapshot = self._orig_snap
        self.workload._status_state = self._orig_state

    def test_no_phase_progress_does_not_abort(self):
        import json as _json

        class FakeClient:
            def post(self, path, payload=None):
                return 202, b"{}", 0.0

            def __init__(self):
                self.calls = 0

            def get(self, path):
                self.calls += 1
                if self.calls > 4:
                    body = _json.dumps(
                        {
                            "state": "completed",
                            "scanned": 5,
                            "matched": 5,
                            "processed": 7,
                            "failed": 0,
                        }
                    ).encode()
                elif self.calls % 2 == 1:
                    body = _json.dumps(
                        {
                            "state": "running",
                            "scanned": 5,
                            "matched": 5,
                            "processed": 7,
                            "failed": 0,
                        }
                    ).encode()
                else:
                    body = _json.dumps(
                        {
                            "state": "completed",
                            "scanned": 5,
                            "matched": 5,
                            "processed": 7,
                            "failed": 0,
                        }
                    ).encode()
                return 200, body, 0.0

        self.workload.probe_phase = lambda *a, **k: {
            "rounds_total": 3,
            "rounds_valid": 3,
            "invalid_reasons": [],
            "progress": False,
        }
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            probe_interval_s=0.0,
            scan_timeout_s=0.1,
            min_valid_rounds=1,
        )
        self.workload.run_scans(cfg, FakeClient(), self.workload.Logger(self.out / "h3.log"))


class ProbeAggregateLoopTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None
        self._orig_probe_phase = self.workload.probe_phase
        self._orig_build = self.workload.build_probe_list
        self._orig_probe_round = self.workload.probe_round
        self._orig_snap = self.workload._status_snapshot
        self._orig_state = self.workload._status_state

    def tearDown(self):
        self.workload.time.sleep = self._orig_sleep
        self.workload.probe_phase = self._orig_probe_phase
        self.workload.build_probe_list = self._orig_build
        self.workload.probe_round = self._orig_probe_round
        self.workload._status_snapshot = self._orig_snap
        self.workload._status_state = self._orig_state

    def _fake_client(self):
        import json as _json

        class FakeClient:
            def post(self, path, payload=None):
                return 202, b"{}", 0.0

            def __init__(self):
                self.calls = 0

            def get(self, path):
                self.calls += 1
                if self.calls > 4:
                    body = _json.dumps(
                        {
                            "state": "completed",
                            "scanned": 5,
                            "matched": 5,
                            "processed": 7,
                            "failed": 0,
                        }
                    ).encode()
                elif self.calls % 2 == 1:
                    body = _json.dumps(
                        {
                            "state": "running",
                            "scanned": 5,
                            "matched": 5,
                            "processed": 7,
                            "failed": 0,
                        }
                    ).encode()
                else:
                    body = _json.dumps(
                        {
                            "state": "completed",
                            "scanned": 5,
                            "matched": 5,
                            "processed": 7,
                            "failed": 0,
                        }
                    ).encode()
                return 200, body, 0.0

        return FakeClient()

    def test_two_repeats_aggregate_pass(self):
        outcomes = iter(
            [
                {"rounds_total": 2, "rounds_valid": 2, "invalid_reasons": [], "progress": True},
                {"rounds_total": 1, "rounds_valid": 1, "invalid_reasons": [], "progress": True},
            ]
        )
        self.workload.probe_phase = lambda *a, **k: next(outcomes)
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            scan_repeats=2,
            probe_interval_s=0.0,
            scan_timeout_s=0.1,
            min_valid_rounds=3,
        )
        self.workload.run_scans(cfg, self._fake_client(), self.workload.Logger(self.out / "h.log"))

    def test_two_repeats_aggregate_fail_names_counts(self):
        outcomes = iter(
            [
                {"rounds_total": 1, "rounds_valid": 1, "invalid_reasons": [], "progress": True},
                {"rounds_total": 1, "rounds_valid": 1, "invalid_reasons": [], "progress": True},
            ]
        )
        self.workload.probe_phase = lambda *a, **k: next(outcomes)
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            scan_repeats=2,
            probe_interval_s=0.0,
            scan_timeout_s=0.1,
            min_valid_rounds=3,
        )
        with self.assertRaises(self.workload.WorkloadError) as ctx:
            self.workload.run_scans(
                cfg, self._fake_client(), self.workload.Logger(self.out / "h.log")
            )
        msg = str(ctx.exception)
        self.assertIn("1, 1", msg)

    def test_zero_valid_repeat_with_enough_total_does_not_abort(self):
        outcomes = iter(
            [
                {"rounds_total": 0, "rounds_valid": 0, "invalid_reasons": [], "progress": False},
                {"rounds_total": 3, "rounds_valid": 3, "invalid_reasons": [], "progress": True},
            ]
        )
        self.workload.probe_phase = lambda *a, **k: next(outcomes)
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            scan_repeats=2,
            probe_interval_s=0.0,
            scan_timeout_s=0.1,
            min_valid_rounds=3,
        )
        self.workload.run_scans(cfg, self._fake_client(), self.workload.Logger(self.out / "h.log"))


class ProbeCachedRowsLoopTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None
        self._orig_build = self.workload.build_probe_list
        self._orig_probe_round = self.workload.probe_round
        self._orig_snap = self.workload._status_snapshot
        self._orig_state = self.workload._status_state

    def tearDown(self):
        self.workload.time.sleep = self._orig_sleep
        self.workload.build_probe_list = self._orig_build
        self.workload.probe_round = self._orig_probe_round
        self.workload._status_snapshot = self._orig_snap
        self.workload._status_state = self._orig_state

    def test_reuses_cached_rows_when_build_empty(self):
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            probe_interval_s=0.0,
            probe_refresh_s=0.0,
            scan_timeout_s=0.1,
            min_valid_rounds=1,
        )
        calls = {"n": 0}

        def build(cfg, client):
            calls["n"] += 1
            if calls["n"] == 1:
                return ["a\tt", "b\tt"]
            return []

        self.workload.build_probe_list = build
        self.workload.probe_round = lambda *a, **k: True
        counter = {"n": 0}

        def snap(out):
            counter["n"] += 1
            state = "running" if counter["n"] <= 12 else "completed"
            return {
                "state": state,
                "scanned": counter["n"],
                "processed": counter["n"],
                "ts": float(counter["n"]),
            }

        def st(out):
            return "running" if counter["n"] < 12 else "completed"

        self.workload._status_snapshot = snap
        self.workload._status_state = st
        cache = []
        logger = self.workload.Logger(self.out / "h.log")
        self.addCleanup(logger.close)
        result = self.workload.probe_phase(
            cfg, object(), logger, self.out / "rounds-run-1", cached_rows=cache
        )
        self.assertGreaterEqual(result["rounds_total"], 2)
        self.assertEqual(cache, ["a\tt", "b\tt"])


class RefreshScanStatusTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()

    def test_refresh_returns_running_and_writes_second_body(self):
        import json as _json

        class FakeClient:
            def __init__(self):
                self.calls = 0

            def get(self, path):
                self.calls += 1
                if self.calls == 1:
                    body = _json.dumps({"state": "completed"}).encode()
                else:
                    body = _json.dumps({"state": "running", "scanned": 1}).encode()
                return 200, body, 0.0

        client = FakeClient()
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            poll_interval_s=0.0,
            scan_timeout_s=0.1,
        )
        logger = self.workload.Logger(self.out / "h.log")
        self.addCleanup(logger.close)
        state = self.workload._refresh_scan_status(cfg, client, self.out, logger, attempts=2)
        self.assertEqual(state, "running")
        self.assertEqual(
            json.loads((self.out / "status-last.json").read_text())["state"], "running"
        )

    def test_refresh_returns_unknown_when_no_running_state(self):
        class FakeClient:
            def get(self, path):
                return 200, b'{"state": "unknown"}', 0.0

        client = FakeClient()
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            poll_interval_s=0.0,
            scan_timeout_s=0.1,
        )
        logger = self.workload.Logger(self.out / "h.log")
        self.addCleanup(logger.close)
        state = self.workload._refresh_scan_status(cfg, client, self.out, logger, attempts=1)
        self.assertEqual(state, "unknown")


class WarmupScanTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self._orig_scroll = self.workload.scroll_probe
        self.scroll_calls = {"n": 0}

        def fake_scroll(cfg, client, timestamp, token, **kw):
            self.scroll_calls["n"] += 1
            return {"scrollbar": [], "rows": [], "data": []}

        self.workload.scroll_probe = fake_scroll
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None
        self._orig_warmup = getattr(self.workload, "warmup_scan", None)
        self._orig_build = self.workload.build_probe_list
        self._orig_state = self.workload._status_state
        self._orig_snap = self.workload._status_snapshot

    def tearDown(self):
        self.workload.scroll_probe = self._orig_scroll
        self.workload.time.sleep = self._orig_sleep
        self.workload.build_probe_list = self._orig_build
        self.workload._status_state = self._orig_state
        self.workload._status_snapshot = self._orig_snap
        if self._orig_warmup is not None:
            self.workload.warmup_scan = self._orig_warmup

    def _client(self):
        class FakeClient:
            def post(self, path, payload=None):
                if path == "/get/prefetch":
                    body = json.dumps({"prefetch": {"timestamp": 42}, "token": "tok"}).encode()
                    return 200, body, 0.0
                return 200, b"{}", 0.0

            def get(self, path, bearer=None, timeout=30.0):
                return 200, b"{}", 0.0

        return FakeClient()

    def _cfg(self, **overrides):
        fields = {"out": self.out, "scan_warmup": 1}
        fields.update(overrides)
        return self.workload.config.WorkloadConfig(**fields)

    def test_warmup_valid_emits_event(self):
        (self.out / "status-last.json").write_text(
            json.dumps({"state": "running", "scanned": 1, "processed": 1})
        )
        cfg = self._cfg()
        rows = ["hash123\ttok456"]
        result = self.workload.warmup_scan(
            cfg,
            self._client(),
            self.out,
            1,
            rows,
        )
        self.assertTrue(result["valid"])
        self.assertIn("prefetch", result["endpoints"])
        self.assertIn("compressed", result["endpoints"])
        self.assertIn("scroll", result["endpoints"])
        self.assertEqual(self.scroll_calls["n"], 1)
        import extract_bench as eb

        ev = eb.parse_events((self.out / "events.jsonl").read_text())
        self.assertEqual(len(ev.warmups), 1)
        self.assertTrue(ev.warmups[0]["valid"])
        self.assertEqual(len(ev.scroll_probes), 0)

    def test_warmup_waits_for_rows(self):
        (self.out / "status-last.json").write_text(
            json.dumps({"state": "running", "scanned": 1, "processed": 1})
        )
        calls = {"n": 0}

        def build(cfg, client):
            calls["n"] += 1
            if calls["n"] < 3:
                return []
            return ["hash123\ttok456"]

        self.workload.build_probe_list = build
        self.workload._status_state = lambda out: "running"
        cfg = self._cfg()
        cache = []
        result = self.workload.warmup_scan(cfg, self._client(), self.out, 1, cache)
        self.assertTrue(result["valid"])
        self.assertEqual(calls["n"], 3)
        self.assertEqual(cache, ["hash123\ttok456"])

    def test_warmup_terminal_before_rows_invalid(self):
        (self.out / "status-last.json").write_text(
            json.dumps({"state": "completed", "scanned": 1, "processed": 1})
        )
        self.workload.build_probe_list = lambda cfg, client: []
        self.workload._status_state = lambda out: "completed"
        cfg = self._cfg()
        result = self.workload.warmup_scan(cfg, self._client(), self.out, 1, [])
        self.assertFalse(result["valid"])
        self.assertEqual(result["reason"], "no probe rows")

    def test_warmup_uses_provided_rows_without_build(self):
        (self.out / "status-last.json").write_text(
            json.dumps({"state": "running", "scanned": 1, "processed": 1})
        )
        calls = {"n": 0}

        def boom(cfg, client):
            calls["n"] += 1
            raise AssertionError("build_probe_list should not be called")

        self.workload.build_probe_list = boom
        cfg = self._cfg()
        rows = ["hash123\ttok456"]
        result = self.workload.warmup_scan(cfg, self._client(), self.out, 1, rows)
        self.assertTrue(result["valid"])
        self.assertEqual(calls["n"], 0)
        self.assertIn("compressed", result["endpoints"])

    def test_warmup_updates_caller_cache(self):
        (self.out / "status-last.json").write_text(
            json.dumps({"state": "running", "scanned": 1, "processed": 1})
        )
        cache = []
        self.workload.build_probe_list = lambda cfg, client: ["built1\ttok", "built2\ttok"]
        cfg = self._cfg()
        result = self.workload.warmup_scan(cfg, self._client(), self.out, 1, cache)
        self.assertTrue(result["valid"])
        self.assertEqual(cache, ["built1\ttok", "built2\ttok"])

    def test_warmup_invalid_terminal_at_end(self):
        (self.out / "status-last.json").write_text(
            json.dumps({"state": "completed", "scanned": 1, "processed": 1})
        )
        cfg = self._cfg()
        rows = ["hash123\ttok456"]
        result = self.workload.warmup_scan(
            cfg,
            self._client(),
            self.out,
            1,
            rows,
        )
        self.assertFalse(result["valid"])

    def test_warmup_zero_skips_in_run_scans(self):
        calls = {"n": 0}

        def fake_warmup(*a, **k):
            calls["n"] += 1
            return {"valid": True, "reason": "ok", "wall_ms": 1, "endpoints": []}

        self.workload.warmup_scan = fake_warmup
        import json as _json

        class FakeClient:
            def __init__(self):
                self.calls = 0

            def post(self, path, payload=None):
                return 202, b"{}", 0.0

            def get(self, path):
                self.calls += 1
                if self.calls == 1:
                    body = _json.dumps(
                        {
                            "state": "running",
                            "scanned": 1,
                            "matched": 1,
                            "processed": 1,
                            "failed": 0,
                        }
                    ).encode()
                else:
                    body = _json.dumps(
                        {
                            "state": "completed",
                            "scanned": 1,
                            "matched": 1,
                            "processed": 1,
                            "failed": 0,
                        }
                    ).encode()
                return 200, body, 0.0

        self.workload.probe_phase = lambda *a, **k: {
            "rounds_total": 1,
            "rounds_valid": 1,
            "invalid_reasons": [],
            "progress": True,
        }
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            scan_warmup=0,
            scan_repeats=1,
            probe_interval_s=0.0,
            scan_timeout_s=0.1,
            scan_stall_timeout_s=0.0,
            min_valid_rounds=1,
        )
        self.workload.run_scans(cfg, FakeClient(), self.workload.Logger(self.out / "h3.log"))
        self.assertEqual(calls["n"], 0)

    def test_warmup_only_run_index_one(self):
        calls = {"n": 0}

        def fake_warmup(*a, **k):
            calls["n"] += 1
            return {"valid": True, "reason": "ok", "wall_ms": 1, "endpoints": []}

        self.workload.warmup_scan = fake_warmup
        import json as _json

        class FakeClient:
            def __init__(self):
                self.calls = 0

            def post(self, path, payload=None):
                return 202, b"{}", 0.0

            def get(self, path):
                self.calls += 1
                if self.calls == 1:
                    body = _json.dumps(
                        {
                            "state": "running",
                            "scanned": 1,
                            "matched": 1,
                            "processed": 1,
                            "failed": 0,
                        }
                    ).encode()
                else:
                    body = _json.dumps(
                        {
                            "state": "completed",
                            "scanned": 1,
                            "matched": 1,
                            "processed": 1,
                            "failed": 0,
                        }
                    ).encode()
                return 200, body, 0.0

        self.workload.probe_phase = lambda *a, **k: {
            "rounds_total": 1,
            "rounds_valid": 1,
            "invalid_reasons": [],
            "progress": True,
        }
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            scan_warmup=1,
            scan_repeats=2,
            probe_interval_s=0.0,
            scan_timeout_s=0.1,
            scan_stall_timeout_s=0.0,
            min_valid_rounds=1,
        )
        self.workload.run_scans(cfg, FakeClient(), self.workload.Logger(self.out / "h4.log"))
        self.assertEqual(calls["n"], 1)


class PhaseGuardExtensionTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.repo = Path(self.tmp.name) / "repo"
        self.repo.mkdir()
        (self.repo / "img_0001.jpg").write_bytes(b"x" * 10)
        self.workload = load_workload()
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None
        self._orig_run_storm = self.workload.run_storm
        self._orig_check_margin = getattr(self.workload, "check_storm_coverage", None)
        self._orig_snapshot = self.workload._status_snapshot
        self._orig_state = self.workload._status_state
        self._orig_post_scan = self.workload.run_post_scan
        self._orig_prepare = self.workload.prepare_images
        self._orig_probe = self.workload.probe_phase

    def tearDown(self):
        self.workload.time.sleep = self._orig_sleep
        self.workload.run_storm = self._orig_run_storm
        self.workload.check_storm_coverage = self._orig_check_margin
        self.workload._status_snapshot = self._orig_snapshot
        self.workload._status_state = self._orig_state
        self.workload.run_post_scan = self._orig_post_scan
        self.workload.prepare_images = self._orig_prepare
        self.workload.probe_phase = self._orig_probe

    def _client(self):
        import json as _json

        class FakeClient:
            def __init__(self):
                self.calls = 0

            def post(self, path, payload=None):
                return 202, b"{}", 0.0

            def get(self, path):
                self.calls += 1
                if self.calls == 1:
                    body = _json.dumps(
                        {
                            "state": "running",
                            "scanned": 5,
                            "matched": 5,
                            "processed": 5,
                            "failed": 0,
                        }
                    ).encode()
                else:
                    body = _json.dumps(
                        {
                            "state": "completed",
                            "scanned": 5,
                            "matched": 5,
                            "processed": 5,
                            "failed": 0,
                        }
                    ).encode()
                return 200, body, 0.0

        return FakeClient()

    def _cfg(self, **overrides):
        fields = {
            "out": self.out,
            "image_repo": self.repo,
            "image_path": self.out / "images",
            "probe_interval_s": 0.0,
            "scan_timeout_s": 0.1,
            "scan_stall_timeout_s": 0.0,
            "min_valid_rounds": 1,
        }
        fields.update(overrides)
        return self.workload.config.WorkloadConfig(**fields)

    def test_storm_concurrent_valid(self):
        self.workload.run_storm = lambda *a, **k: {"done": 1, "total": 1, "wall": 0.1, "end": 0.0}
        self.workload.check_storm_coverage = lambda *a, **k: None
        self.workload._status_snapshot = lambda out: {
            "state": "running",
            "scanned": 1,
            "processed": 1,
            "ts": 1.0,
        }
        cfg = self._cfg(workload="storm-read")
        self.workload.run_scans(cfg, self._client(), self.workload.Logger(self.out / "h.log"))

    def test_post_scan_invalid(self):
        self.workload.prepare_images = lambda cfg, logger: (1, 10)
        self.workload.probe_phase = lambda *a, **k: {
            "rounds_total": 1,
            "rounds_valid": 1,
            "invalid_reasons": [],
            "progress": True,
        }
        self.workload.run_post_scan = lambda cfg, client, logger: None
        self.workload._status_snapshot = lambda out: {
            "state": "running",
            "scanned": 1,
            "processed": 1,
            "ts": 1.0,
        }
        self.workload._status_state = lambda out: "completed"
        client = self._client()
        cfg = self._cfg(workload="control")
        self.workload.HttpClient = lambda base_url, cookie_jar: client
        self.workload.run_scans(cfg, client, self.workload.Logger(self.out / "h4.log"))
        rc = self.workload.run_workload(cfg)
        self.assertEqual(rc, 2)


class StormSustainedTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None

    def tearDown(self):
        self.workload.time.sleep = self._orig_sleep

    def _cfg(self, **overrides):
        fields = {
            "out": self.out,
            "workload": "storm-edit",
            "storm_clients": 2,
            "storm_ops": 2,
            "storm_min_coverage_pct": 50.0,
            "probe_interval_s": 0.0,
            "scan_timeout_s": 0.0,
        }
        fields.update(overrides)
        return self.workload.config.WorkloadConfig(**fields)

    def test_wait_for_rows_min(self):
        calls = {"n": 0}

        def build(cfg, client):
            calls["n"] += 1
            if calls["n"] < 2:
                return ["a\tt"]
            return ["a\tt", "b\tt", "c\tt", "d\tt", "e\tt"]

        self.workload.build_probe_list = build
        (self.out / "rounds").mkdir(parents=True, exist_ok=True)
        lines = self.workload._wait_for_rows(
            self._cfg(),
            self._client() if False else self._make_client(),
            self.workload.Logger(self.out / "h3.log"),
            None,
            min_rows=5,
        )
        self.assertGreaterEqual(len(lines), 5)

    def _make_client(self):
        class FakeClient:
            def post(self, path, payload=None, timeout=30.0):
                if path == "/get/prefetch":
                    return (
                        200,
                        json.dumps({"prefetch": {"timestamp": 42}, "token": "tok"}).encode(),
                        0.0,
                    )
                return 200, b"{}", 0.0

            def put(self, path, payload=None, timeout=30.0):
                return 200, b"{}", 0.0

            def get(self, path, bearer=None, timeout=30.0):
                return 200, b"{}", 0.0

        return FakeClient()

    def test_coverage_below_threshold_fails(self):
        summary = {"done": 10, "total": 10, "active_wall": 1.0}
        with self.assertRaises(self.workload.WorkloadError):
            self.workload.check_storm_coverage(summary, 0.0, 10.0, 50.0)

    def test_running_terminal_accepted_for_storm(self):
        import tempfile
        from pathlib import Path

        out = Path(tempfile.mkdtemp()) / "cell"
        out.mkdir(parents=True)
        snaps = iter(
            [
                {"state": "running", "scanned": 1, "processed": 1, "ts": 1.0},
                {"state": "completed", "scanned": 2, "processed": 2, "ts": 2.0},
            ]
        )
        guard = self.workload.ScanPhase(
            out,
            "storm",
            expected_state="running",
            allow_terminal_end=True,
            snapshot=lambda: next(snaps),
        )
        guard.start()
        result = guard.end()
        self.assertTrue(result["valid"])


class StormRowFloorTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()

    def test_128_clients_floor_16(self):
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            workload="storm-edit-16n",
            storm_clients=128,
            storm_ops=10,
            expected_entries=100,
        )
        # min(cfg.expected_entries, max(8, clients//8)) = min(100,16)=16
        self.assertEqual(min(cfg.expected_entries, max(8, cfg.storm_clients // 8)), 16)

    def test_8_clients_floor_8_or_10(self):
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            workload="storm-edit-8",
            storm_clients=8,
            storm_ops=10,
            expected_entries=100,
        )
        self.assertEqual(min(cfg.expected_entries, max(8, cfg.storm_clients // 8)), 8)


class StormTimestampRefreshTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None

    def tearDown(self):
        self.workload.time.sleep = self._orig_sleep


class ConcurrentMinimumCapTest(unittest.TestCase):
    """A short scan must not demand an unreachable number of storm ops."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None

    def tearDown(self):
        self.workload.time.sleep = self._orig_sleep

    def test_eight_and_128_clients_derive_the_expected_minimums(self):
        helper = self.workload.storm_minimum_total
        self.assertEqual(helper(8, 10, True), 80)
        self.assertEqual(helper(128, 10, True), 256)
        self.assertEqual(helper(128, 10, False), 1280)
        self.assertEqual(helper(2, 2, True), 4)

    def test_cap_keeps_at_least_one_op_per_client_or_80(self):
        helper = self.workload.storm_minimum_total
        self.assertEqual(helper(128, 1, True), 128)
        self.assertEqual(helper(256, 1, True), 256)

    def test_1072_ops_pass_the_capped_minimum(self):
        summary = {"done": 1072, "total": 256, "active_wall": 1.4, "wall": 1.4}

        self.workload.check_storm_coverage(summary, 0.0, 1.4, 50.0)

        self.assertEqual(summary["coverage_pct"], 100.0)

    def test_below_the_capped_minimum_fails_and_names_it(self):
        summary = {"done": 255, "total": 256, "active_wall": 1.4, "wall": 1.4}

        with self.assertRaises(self.workload.WorkloadError) as caught:
            self.workload.check_storm_coverage(summary, 0.0, 1.4, 50.0)

        self.assertIn("minimum=256", str(caught.exception))
        self.assertIn("done=255", str(caught.exception))

    def test_concurrent_run_reports_the_capped_total(self):
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            workload="storm-edit-16n",
            storm_clients=128,
            storm_ops=10,
            storm_min_coverage_pct=50.0,
            probe_interval_s=0.0,
            scan_timeout_s=0.0,
        )
        seen = {"ops": 0}

        class FakeClient:
            def post(self, path, payload=None, timeout=30.0):
                return (
                    200,
                    b'{"prefetch":{"timestamp":42},"token":"tok"}',
                    0.0,
                )

            def put(self, path, payload=None, timeout=30.0):
                seen["ops"] += 1
                return 200, b"{}", 0.0

            def get(self, path, bearer=None, timeout=30.0):
                return 200, b"{}", 0.0

        self.workload.build_probe_list = lambda cfg, client: [f"h{i}\ttok" for i in range(5)]
        (self.out / "rounds").mkdir(parents=True, exist_ok=True)
        scan_active = threading.Event()
        scan_active.set()
        client = FakeClient()
        original_put = client.put

        def put(path, payload=None, timeout=30.0):
            result = original_put(path, payload, timeout)
            if seen["ops"] >= 300:
                scan_active.clear()
            return result

        client.put = put

        summary = self.workload.run_storm(
            cfg,
            client,
            self.workload.Logger(self.out / "cap.log"),
            self.out / "rounds",
            1,
            scan_active,
        )

        self.assertEqual(summary["total"], 256)
        self.assertGreaterEqual(summary["done"], 256)
        self.assertLess(seen["ops"], 1280)
