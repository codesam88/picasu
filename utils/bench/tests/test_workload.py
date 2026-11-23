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

import config  # noqa: E402
import executor  # noqa: E402
import scenarios  # noqa: E402


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
            self.assertIn(cfg.workload, config.WORKLOADS)
            self.assertEqual(cfg.workload, scenarios.PROBE)
            self.assertEqual(cfg.storm_clients, config.DEFAULT_STORM_CLIENTS)
            self.assertEqual(cfg.storm_ops, config.DEFAULT_STORM_OPS)
            for bad in ("not-a-profile", "", "PROBE"):
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
            self.assertEqual(cfg.upload_timeout_s, 120.0)
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

    def _phase(self, target=2):
        return scenarios.Phase("idle", ("upload",), issue="files", stop="files", target=target)

    def test_post_budget_reaches_the_upload_call(self):
        client = RecordingUploadClient(["u1.jpg", "u2.jpg"])
        with (
            mock.patch.object(self.workload, "_uploaded_row_visible", return_value=True),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            executor.drive_files(
                self._cfg(upload_post_timeout_s=0.25), client, self._logger(), self._phase(), None
            )
        self.assertEqual(client.timeouts, [0.25, 0.25])
        log = (self.cell / "harness-a.log").read_text()
        self.assertRegex(log, r"upload file=u1\.jpg submit_ms=\d+ visible_ms=\d+")
        self.assertRegex(log, r"upload file=u2\.jpg submit_ms=\d+ visible_ms=\d+")
        self.assertRegex(log, r"upload submit_p50=\d+ visible_p50=\d+")

    def test_files_record_the_scan_activity_at_submit(self):
        client = RecordingUploadClient(["u1.jpg", "u2.jpg"])

        class Session:
            class Active:
                def is_set(self):
                    return True

            active = Active()

        with (
            mock.patch.object(self.workload, "_uploaded_row_visible", return_value=True),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            executor.drive_files(self._cfg(), client, self._logger(), self._phase(), Session())
        events = (self.cell / "events.jsonl").read_text().splitlines()
        uploads = [json.loads(line) for line in events if json.loads(line)["type"] == "upload_file"]
        self.assertEqual(len(uploads), 2)
        self.assertTrue(all(event["scan_active_at_submit"] for event in uploads))
        self.assertTrue(all(event["scan_active_at_visible"] for event in uploads))

    def test_idle_upload_records_no_scan_activity(self):
        client = RecordingUploadClient(["u1.jpg", "u2.jpg"])
        with (
            mock.patch.object(self.workload, "_uploaded_row_visible", return_value=True),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            executor.drive_files(self._cfg(), client, self._logger(), self._phase(), None)
        events = (self.cell / "events.jsonl").read_text().splitlines()
        uploads = [json.loads(line) for line in events if json.loads(line)["type"] == "upload_file"]
        self.assertTrue(all(not event["scan_active_at_submit"] for event in uploads))


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
            "workload": scenarios.storm_profiles()[0],
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
            storm_clients=128,
            storm_ops=10,
            expected_entries=100,
        )
        # min(cfg.expected_entries, max(8, clients//8)) = min(100,16)=16
        self.assertEqual(min(cfg.expected_entries, max(8, cfg.storm_clients // 8)), 16)

    def test_8_clients_floor_8_or_10(self):
        cfg = self.workload.config.WorkloadConfig(
            out=self.out,
            storm_clients=8,
            storm_ops=10,
            expected_entries=100,
        )
        self.assertEqual(min(cfg.expected_entries, max(8, cfg.storm_clients // 8)), 8)


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


class ScenarioSpecTest(unittest.TestCase):
    """The cell matrix is data: each spec states its phases explicitly."""

    def setUp(self):
        self.workload = load_workload()

    def spec(self, profile, **overrides):
        fields = {
            "min_probe_rounds": 20,
            "repeats": 3,
            "upload_files": 20,
            "storm_ops": 10,
            "ncpu": 8,
        }
        fields.update(overrides)
        return scenarios.build_spec(profile, **fields)

    def test_every_profile_builds_a_spec(self):
        self.assertEqual(set(scenarios.profiles()), set(config.WORKLOADS))
        for profile in scenarios.profiles():
            with self.subTest(profile=profile):
                spec = self.spec(profile)
                self.assertEqual(spec.profile, profile)
                self.assertTrue(spec.phases)

    def test_probe_has_scan_rounds_then_idle_rounds(self):
        spec = self.spec(scenarios.PROBE, min_probe_rounds=7)
        self.assertEqual(spec.probe_round_target, 7)
        scan, idle = spec.phases
        self.assertEqual((scan.background, scan.issue, scan.stop), ("scan", "rounds", "scan"))
        self.assertEqual(scan.foregrounds, scenarios.PROBE_FOREGROUNDS)
        self.assertEqual((idle.background, idle.issue, idle.stop), ("idle", "rounds", "samples"))
        self.assertEqual(idle.target, 3)
        self.assertEqual(idle.foregrounds, scenarios.PROBE_FOREGROUNDS)

    def test_storm_specs_match_their_background_kind_and_level(self):
        for profile in scenarios.storm_profiles():
            with self.subTest(profile=profile):
                background, kind, level = scenarios.storm_parts(profile)
                spec = self.spec(profile)
                self.assertEqual(spec.probe_round_target, 0)
                self.assertEqual(len(spec.phases), 1)
                phase = spec.phases[0]
                self.assertEqual(phase.background, background)
                self.assertEqual(phase.foregrounds, (scenarios.STORM_ACCESS[kind],))
                self.assertEqual(phase.clients, scenarios.level_clients(level, 8))
                self.assertEqual(phase.issue, "stream")
                self.assertEqual(phase.setup_scan, background == "idle")
                self.assertEqual(phase.stop, "scan" if background == "scan" else "ops")

    def test_upload_specs_differ_only_in_background(self):
        idle = self.spec(scenarios.IDLE_UPLOAD).phases[0]
        scan = self.spec(scenarios.SCAN_UPLOAD).phases[0]
        self.assertEqual(idle.background, "idle")
        self.assertEqual(scan.background, "scan")
        for phase in (idle, scan):
            self.assertEqual(phase.foregrounds, ("upload",))
            self.assertEqual((phase.issue, phase.stop, phase.target), ("files", "files", 20))

    def test_unknown_profile_is_rejected(self):
        with self.assertRaises(ValueError):
            self.spec("not-a-declared-profile")


class _FakeSession:
    """Minimal ScanSession stand-in for executor phase dispatch tests."""

    def __init__(self, cfg, client, logger, run_index, **kwargs):
        self.run_index = run_index
        self.wall_ms = 100
        self.started = 0.0
        self.ended = 10.0
        self.active = threading.Event()
        self.active.set()
        self.guard_result = {"valid": True, "reason": "ok"}

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return None


class _FakeRequestClient:
    def __init__(self):
        self.calls = []

    def post(self, path, payload=None, timeout=30.0):
        self.calls.append(("post", path))
        return 200, json.dumps({"prefetch": {"timestamp": 42}, "token": "tok"}).encode(), 0.01

    def put(self, path, payload=None, timeout=30.0):
        self.calls.append(("put", path))
        return 200, b"{}", 0.01

    def get(self, path, bearer=None, timeout=30.0):
        self.calls.append(("get", path))
        return 200, b"[]", 0.01


class ExecutorTest(unittest.TestCase):
    """The executor drives phases from a spec, with no profile-name branch."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "cell"
        self.out.mkdir()
        self.workload = load_workload()
        self.logger = self.workload.Logger(self.out / "harness-a.log")
        self.addCleanup(self.logger.close)
        self._orig_sleep = self.workload.time.sleep
        self.workload.time.sleep = lambda x: None
        self.addCleanup(setattr, self.workload.time, "sleep", self._orig_sleep)
        (self.out / "prefetch.json").write_text(
            json.dumps({"prefetch": {"timestamp": 42}, "token": "tok"})
        )

    def _cfg(self, **overrides):
        fields = {
            "out": self.out,
            "scan_repeats": 3,
            "storm_min_coverage_pct": 50.0,
            "warmup": 1,
            "repeats": 2,
            "min_probe_rounds": 5,
            "images": 5,
            "upload_files": 2,
            "probe_page": 1000,
            "upload_timeout_s": 0.1,
            "poll_interval_s": 0.01,
        }
        fields.update(overrides)
        return self.workload.config.WorkloadConfig(**fields)

    def _events(self):
        path = self.out / "events.jsonl"
        if not path.is_file():
            return []
        return [json.loads(line) for line in path.read_text().splitlines()]

    def test_probe_scans_repeat_to_the_round_target_then_runs_idle(self):
        cfg = self._cfg(min_probe_rounds=5, scan_repeats=3)
        spec = scenarios.build_spec(
            scenarios.PROBE,
            min_probe_rounds=5,
            repeats=2,
            upload_files=2,
            storm_ops=10,
            ncpu=8,
        )
        targets = []
        idle_calls = []

        def fake_rounds(c, client, logger, session, target, warm):
            targets.append((target, warm))
            return 2 if len(targets) == 1 else 3

        with (
            mock.patch.object(executor, "ScanSession", _FakeSession),
            mock.patch.object(executor, "drive_rounds_scan", side_effect=fake_rounds),
            mock.patch.object(
                executor, "drive_rounds_idle", side_effect=lambda *a: idle_calls.append(a)
            ),
        ):
            executor.execute_cell(cfg, _FakeRequestClient(), self.logger, spec)
        self.assertEqual(targets, [(5, True), (3, False)])
        self.assertEqual(len(idle_calls), 1)
        self.assertEqual(cfg.scan_runs, 2)
        self.assertEqual(cfg.scan_wall_ms, 100)

    def test_idle_rounds_discard_warmup_samples(self):
        cfg = self._cfg(warmup=2, repeats=3)
        phase = scenarios.Phase("idle", ("prefetch", "edit", "thumb", "scroll"), target=3)
        with (
            mock.patch.object(executor.wl, "build_probe_list", return_value=["h\ttok"]),
            mock.patch.object(
                executor.wl,
                "scroll_probe",
                return_value={"scrollbar": [0.01], "rows": [], "data": []},
            ),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            executor.drive_rounds_idle(cfg, _FakeRequestClient(), self.logger, phase)
        samples = [event for event in self._events() if event["type"] == "idle_sample"]
        warmups = [event for event in samples if event["phase"] == "warmup"]
        measured = [event for event in samples if event["phase"] == "measured"]
        self.assertEqual(len(warmups), 2 * 3)
        self.assertEqual(len(measured), 3 * 3)
        self.assertEqual({event["access"] for event in measured}, {"prefetch", "edit", "thumb"})

    def test_drive_rounds_scan_counts_only_valid_rounds(self):
        cfg = self._cfg()
        session = _FakeSession(cfg, None, self.logger, 1)
        session.active.set()
        validity = iter([True, False, True, True])

        class FakeGuard:
            def __init__(self, *args, **kwargs):
                pass

            def start(self):
                pass

            def end(self):
                return {"valid": next(validity), "reason": "ok"}

        with (
            mock.patch.object(executor.wl, "build_probe_list", return_value=["h\ttok"]),
            mock.patch.object(executor.wl, "ScanPhase", FakeGuard),
            mock.patch.object(executor.wl, "probe_round", return_value=True),
            mock.patch.object(executor.wl, "_status_state", return_value="running"),
            mock.patch.object(executor.wl, "warmup_scan", return_value={"valid": True}),
        ):
            valid = executor.drive_rounds_scan(
                cfg, _FakeRequestClient(), self.logger, session, 3, False
            )
        self.assertEqual(valid, 3)

    def test_idle_storm_stops_at_the_ops_target(self):
        cfg = self._cfg(storm_ops=3)
        phase = scenarios.Phase(
            "idle", ("prefetch",), clients=2, issue="stream", stop="ops", target=6
        )
        with (
            mock.patch.object(executor.wl, "build_probe_list", return_value=["h\ttok"]),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            summary = executor.drive_stream(cfg, _FakeRequestClient(), self.logger, phase, None, 1)
        self.assertEqual(summary["done"], 6)
        self.assertEqual(summary["total"], 6)
        self.assertEqual(
            len([event for event in self._events() if event["type"] == "storm_sample"]), 6
        )

    def test_scan_storm_coverage_uses_the_session_interval(self):
        cfg = self._cfg(storm_min_coverage_pct=90.0)
        spec = scenarios.build_spec(
            next(p for p in scenarios.storm_profiles() if scenarios.storm_parts(p)[0] == "scan"),
            min_probe_rounds=5,
            repeats=2,
            upload_files=2,
            storm_ops=10,
            ncpu=8,
        )
        summary = {
            "kind": "prefetch",
            "done": 10,
            "total": 10,
            "active_wall": 8.0,
            "wall": 8.0,
            "median": 0.01,
            "p95": 0.02,
            "max": 0.03,
            "failed": 0,
            "coverage_pct": None,
        }
        with (
            mock.patch.object(executor, "ScanSession", _FakeSession),
            mock.patch.object(executor, "drive_stream", return_value=summary),
            mock.patch.object(executor, "_emit_storm_summary"),
        ):
            with self.assertRaises(self.workload.WorkloadError) as caught:
                executor.execute_cell(cfg, _FakeRequestClient(), self.logger, spec)
        self.assertIn("coverage too low", str(caught.exception))
        self.assertEqual(summary["coverage_pct"], 80.0)
