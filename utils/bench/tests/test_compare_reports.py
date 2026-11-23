#!/usr/bin/env python3
"""Tests for compare_reports.py (long schema)."""

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))

import battery  # noqa: E402
import compare_reports as cr  # noqa: E402


def write_long(path: Path, rows: list, header=None, cell_meta=None):
    """Write a long report: header, optional cell metadata, then rows."""
    import json

    header = header or list(battery.REPORT_COLUMNS)
    content = ["\t".join(header)]
    if cell_meta:
        content.append("# cell: " + json.dumps(cell_meta, sort_keys=True))
    for row in rows:
        content.append("\t".join(str(row.get(col, "n/a")) for col in header))
    path.write_text("\n".join(content) + "\n")
    return path


def make_row(
    cell,
    family="latency",
    background="idle",
    subject="prefetch",
    clients=1,
    stat="p50",
    value="100",
    unit="ms",
):
    return {
        "cell": cell,
        "family": family,
        "background": background,
        "subject": subject,
        "clients": clients,
        "stat": stat,
        "value": value,
        "unit": unit,
        "n_samples": "5",
        "n_clusters": "2",
        "ci_low": "n/a",
        "ci_high": "n/a",
    }


class CompareReportsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def _basic(self):
        base = self.root / "base.tsv"
        new = self.root / "new.tsv"
        rows_b = [make_row("c1", value="100")]
        rows_n = [make_row("c1", value="110")]
        write_long(base, rows_b)
        write_long(new, rows_n)
        return base, new

    def test_ok_flag(self):
        base, new = self._basic()
        rows, counts = cr.compare(base, new)
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["flag"], "ok")

    def test_moved_flag(self):
        base, new = self._basic()
        new.write_text(new.read_text().replace("110", "300"))
        rows, counts = cr.compare(base, new)
        self.assertEqual(rows[0]["flag"], "moved")

    def test_missing_flag(self):
        base, new = self._basic()
        new.write_text(new.read_text().replace("c1", "c2"))
        rows, counts = cr.compare(base, new)
        self.assertTrue(any(r["flag"] == "missing" for r in rows))

    def test_new_flag(self):
        base, new = self._basic()
        new.write_text(new.read_text().replace("c1", "c2"))
        rows, counts = cr.compare(base, new)
        self.assertTrue(any(r["flag"] == "new" for r in rows))

    def test_na_flag(self):
        base, new = self._basic()
        new.write_text(new.read_text().replace("110", "n/a"))
        rows, counts = cr.compare(base, new)
        self.assertEqual(rows[0]["flag"], "n/a")

    def test_both_absent_is_na(self):
        base = self.root / "base.tsv"
        new = self.root / "new.tsv"
        write_long(base, [make_row("c1", value="n/a")])
        write_long(new, [make_row("c1", value="n/a")])
        rows, _counts = cr.compare(base, new)
        self.assertEqual(rows[0]["flag"], "n/a")

    def test_tolerance_uses_unit_floor(self):
        self.assertEqual(cr.tolerance_for("ms", 100), 50.0)
        self.assertEqual(cr.tolerance_for("ms", 1), 1.0)
        self.assertEqual(cr.tolerance_for("s", 10), 5.0)
        self.assertEqual(cr.tolerance_for("us", 100000), 50000.0)
        self.assertEqual(cr.tolerance_for("MB", 10), 5.0)
        self.assertEqual(cr.tolerance_for("unknown", 10), 5.0)

    def test_scalar_value_helper(self):
        self.assertEqual(cr._scalar_value(5), 5.0)
        self.assertIsNone(cr._scalar_value("1,3,2"))
        self.assertIsNone(cr._scalar_value("n/a"))
        self.assertIsNone(cr._scalar_value("abc"))

    def test_cli_writes_tsv(self):
        base, new = self._basic()
        tsv = self.root / "out.tsv"
        out = io.StringIO()
        err = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = cr.main([str(base), str(new), "--tsv", str(tsv)])
        self.assertEqual(rc, 0)
        self.assertTrue(tsv.is_file())

    def test_strict_clean_exit(self):
        base, new = self._basic()
        rc = cr.main([str(base), str(new), "--strict"])
        self.assertEqual(rc, 0)

    def test_strict_exit(self):
        base, new = self._basic()
        new.write_text(new.read_text().replace("110", "n/a"))
        rc = cr.main([str(base), str(new), "--strict"])
        self.assertEqual(rc, 1)

    def test_missing_key_reporting(self):
        base, new = self._basic()
        new.write_text(new.read_text().replace("c1", "c2"))
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            rows, counts = cr.compare(base, new)
        self.assertIn("missing", err.getvalue() + str(counts))

    def test_identity_refusal(self):
        base = self.root / "base.tsv"
        new = self.root / "new.tsv"
        meta = {"identity": {"level": "gate", "images": 100, "clients": 1}, "validity": {}}
        write_long(base, [make_row("c1", value="100")], cell_meta=meta)
        meta2 = {"identity": {"level": "gate", "images": 200, "clients": 1}, "validity": {}}
        write_long(new, [make_row("c1", value="110")], cell_meta=meta2)
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            rows, counts = cr.compare(base, new)
        self.assertIn("refused", err.getvalue())
        # refused cell should not be compared
        self.assertEqual(len(rows), 0)

    def test_baselines_input(self):
        baseline = self.root / "accepted.tsv"
        brow = make_row("c1", value="100")
        brow["recorded"] = "old"
        brow["note"] = "seed"
        line = "\t".join(str(brow.get(c, "n/a")) for c in battery.BASELINE_COLUMNS)
        baseline.write_text("\t".join(battery.BASELINE_COLUMNS) + "\n" + line + "\n")
        report = self.root / "new.tsv"
        write_long(report, [make_row("c1", value="100")])
        rows, counts = cr.compare(baseline, report)
        self.assertTrue(len(rows) >= 1)


if __name__ == "__main__":
    unittest.main()
