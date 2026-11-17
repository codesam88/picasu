#!/usr/bin/env python3
"""Tests for bench.py unified CLI shell. Stdlib only."""

import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))


def load_bench():
    import bench

    return bench


class BenchDispatchTests(unittest.TestCase):
    def setUp(self):
        self.bench = load_bench()

    def test_exit_code_passthrough(self):
        alias = self.bench.MODULE_ALIASES["extract"]
        mod = getattr(self.bench, alias)
        original_main = mod.main
        original_modules_extract = self.bench.MODULES.get("extract")
        try:
            for code in (0, 1, 2, 7, 130):
                fake = mock.Mock(return_value=code)
                mod.main = fake
                self.bench.MODULES["extract"] = mod
                rc = self.bench.main(["extract", "x"])
                self.assertEqual(rc, code)
        finally:
            mod.main = original_main
            self.bench.MODULES["extract"] = original_modules_extract


class BenchStatusTests(unittest.TestCase):
    def setUp(self):
        self.bench = load_bench()
        self.tmp = Path(tempfile.mkdtemp())

    def _cell(self, name):
        d = self.tmp / name
        d.mkdir(parents=True, exist_ok=True)
        return d

    def test_states(self):
        self._cell("queued-cell-rep1")
        r = self._cell("running-cell-rep1")
        (r / "status-last.json").write_text("{}")
        (r / "status-final.json").write_text("{}")
        now = time.time()
        os.utime(r / "status-last.json", (now + 5, now + 5))
        os.utime(r / "status-final.json", (now, now))

        f = self._cell("finished-cell-rep1")
        (f / "status-final.json").write_text(
            __import__("json").dumps({"state": "completed", "rc": 3})
        )

        x = self._cell("failed-cell-rep1")
        (x / "diag-orchestrator").mkdir()

        rc = self.bench.main(["status", str(self.tmp)])
        self.assertEqual(rc, 0)
        import io
        from contextlib import redirect_stdout

        buf = io.StringIO()
        with redirect_stdout(buf):
            self.bench._status([str(self.tmp)])
        out = buf.getvalue()
        self.assertIn("queued-cell-rep1: queued", out)
        self.assertIn("running-cell-rep1: running", out)
        self.assertIn("finished-cell-rep1: finished rc=3", out)
        self.assertIn("failed-cell-rep1: failed", out)

    def test_unreadable_parent_returns_1(self):
        f = self.tmp / "notadir"
        f.write_text("x")
        rc = self.bench.main(["status", str(f)])
        self.assertEqual(rc, 1)

    def test_empty_parent_returns_0(self):
        empty = self.tmp / "empty"
        empty.mkdir()
        rc = self.bench.main(["status", str(empty)])
        self.assertEqual(rc, 0)
        import io
        from contextlib import redirect_stdout

        buf = io.StringIO()
        with redirect_stdout(buf):
            self.bench._status([str(empty)])
        self.assertIn("no cells yet", buf.getvalue())


class BenchRunRoutingTest(unittest.TestCase):
    def setUp(self):
        self.bench = load_bench()

    def test_run_routes_to_battery_run(self):
        fake = mock.Mock(return_value=0)
        self.bench.battery.main = fake
        rc = self.bench.main(["run", "cell", "--out", "/tmp/x"])
        self.assertEqual(rc, 0)
        fake.assert_called_once_with(["run", "cell", "--out", "/tmp/x"])


if __name__ == "__main__":
    unittest.main()


class CellMetricsTest(unittest.TestCase):
    """`cell-metrics.json` records process peak and snapshot-store size."""

    def test_record_cell_metrics_reads_vm_hwm_and_temp_db(self):
        import json
        from types import SimpleNamespace

        import run_cell

        out = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(out, ignore_errors=True))
        (out / "data" / "db").mkdir(parents=True)
        payload = b"x" * 2_048_000
        (out / "data" / "db" / "temp_db.redb").write_bytes(payload)
        (out / "launcher-info.json").write_text(json.dumps({"pid": os.getpid()}))

        run_cell.record_cell_metrics(SimpleNamespace(out=out))

        document = json.loads((out / "cell-metrics.json").read_text())
        self.assertEqual(document["temp_db_bytes"], len(payload))
        self.assertEqual(document["temp_db_mb"], 2.0)
        self.assertGreater(document["vm_hwm_mb"], 0)

    def test_missing_temp_db_leaves_the_size_unrecorded(self):
        import json
        from types import SimpleNamespace

        import run_cell

        out = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(out, ignore_errors=True))

        run_cell.record_cell_metrics(SimpleNamespace(out=out))

        document = json.loads((out / "cell-metrics.json").read_text())
        self.assertNotIn("temp_db_mb", document)
        self.assertNotIn("temp_db_bytes", document)
