#!/usr/bin/env python3
"""Tests for bench.py unified CLI shell. Stdlib only."""

import importlib.util
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

BENCH = Path(__file__).resolve().parent.parent


def load_bench():
    spec = importlib.util.spec_from_file_location("bench_cli_test", BENCH / "bench.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class BenchDispatchTests(unittest.TestCase):
    def setUp(self):
        self.bench = load_bench()
        self.fakes = {}
        self._orig_mains = {}
        for cmd in ("cell", "battery", "extract", "seed", "server", "prepare", "preflight"):
            alias = self.bench.MODULE_ALIASES[cmd]
            mod = getattr(self.bench, alias)
            self._orig_mains[alias] = getattr(mod, "main", None)
            fake = mock.Mock(return_value=0)
            mod.main = fake
            setattr(self.bench, alias, mod)
            self.bench.MODULES[cmd] = mod
            self.fakes[cmd] = fake

    def tearDown(self):
        for alias, orig in self._orig_mains.items():
            mod = getattr(self.bench, alias, None)
            if mod is not None and orig is not None:
                mod.main = orig

    def test_dispatch_table_maps_to_right_modules(self):
        expected = {
            "cell": "run_cell",
            "battery": "battery",
            "extract": "extract_bench",
            "seed": "seed_images",
            "server": "launch_backend",
            "prepare": "run_prepare",
            "preflight": "preflight",
        }
        for cmd, alias in expected.items():
            self.assertIn(cmd, self.bench.DISPATCH)
            self.assertEqual(self.bench.MODULE_ALIASES[cmd], alias)

    def test_argv_forwarding_byte_identical(self):
        for cmd in ("cell", "battery", "extract", "seed", "server", "prepare", "preflight"):
            fake = self.fakes[cmd]
            fake.reset_mock()
            argv = ["--foo", "bar baz", "--n=5", "positional"]
            rc = self.bench.main([cmd] + argv)
            self.assertEqual(rc, 0)
            fake.assert_called_once_with(argv)

    def test_exit_code_passthrough(self):
        for code in (0, 1, 2, 7, 130):
            fake = mock.Mock(return_value=code)
            alias = self.bench.MODULE_ALIASES["cell"]
            mod = getattr(self.bench, alias)
            mod.main = fake
            self.bench.MODULES["cell"] = mod
            rc = self.bench.main(["cell", "x"])
            self.assertEqual(rc, code)

    def test_status_unknown_parent_returns_1(self):
        rc = self.bench.main(["status", "/no/such/parent/xyz"])
        self.assertEqual(rc, 1)


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


if __name__ == "__main__":
    unittest.main()


class BenchRunRoutingTest(unittest.TestCase):
    def setUp(self):
        self.bench = load_bench()

    def test_run_routes_to_battery_run(self):
        fake = mock.Mock(return_value=0)
        self.bench.battery.main = fake
        rc = self.bench.main(["run", "cell", "--out", "/tmp/x"])
        self.assertEqual(rc, 0)
        fake.assert_called_once_with(["run", "cell", "--out", "/tmp/x"])
