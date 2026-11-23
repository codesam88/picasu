#!/usr/bin/env python3
"""Battery parents and environment records."""

import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))

import battery  # noqa: E402
import render_baselines  # noqa: E402


class ResolveParentsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def rep(self, parent, cell="cell", n=1):
        (self.root / parent / f"{cell}-rep{n}").mkdir(parents=True)

    def test_direct_parent_passes_through(self):
        self.rep("run1")
        found, err = battery._resolve_parents([str(self.root / "run1")])
        self.assertIsNone(err)
        self.assertEqual(found, [self.root / "run1"])

    def test_run_root_expands_to_contained_batteries(self):
        self.rep("root/first")
        self.rep("root/second", n=2)
        (self.root / "notes.txt").write_text("not a battery")
        found, err = battery._resolve_parents([str(self.root / "root")])
        self.assertIsNone(err)
        self.assertEqual(found, [self.root / "root/first", self.root / "root/second"])

    def test_missing_path_reports_an_error(self):
        found, err = battery._resolve_parents([str(self.root / "absent")])
        self.assertEqual(found, [])
        self.assertIn("not a battery directory", err)

    def test_empty_dir_yields_no_batteries(self):
        (self.root / "empty").mkdir()
        found, err = battery._resolve_parents([str(self.root / "empty")])
        self.assertEqual(found, [])
        self.assertIn("no batteries", err)


class EnvMergeTest(unittest.TestCase):
    def record(self, commit="abc1234", started="2026-11-20T10:00:00+00:00", **over):
        record = {"started_utc": started, "host": "h", "git_commit": commit}
        record.update(over)
        return record

    def test_same_run_replaces_and_other_runs_carry_over(self):
        old = '# env: {"git_commit": "abc", "started_utc": "2026-11-20T10:00:00+00:00", "host": "old"}'
        merged = battery.merge_env_lines(
            [old], [self.record(commit="abc", started="2026-11-20T10:00:00+00:00")]
        )
        self.assertEqual(len(merged), 1)
        self.assertIn('"host": "h"', merged[0])

    def test_distinct_runs_accumulate(self):
        merged = battery.merge_env_lines(
            [],
            [
                self.record(commit="abc", started="2026-11-20T10:00:00+00:00"),
                self.record(commit="def", started="2026-11-20T11:00:00+00:00"),
            ],
        )
        self.assertEqual(len(merged), 2)


class EnvRecordsTest(unittest.TestCase):
    def test_comment_lines_carry_the_environment(self):
        with tempfile.TemporaryDirectory() as tmp:
            tsv = Path(tmp) / "baselines.tsv"
            tsv.write_text('# comment\n# env: {"git_commit": "abc", "started_utc": "2026-11-20"}\n')
            records = render_baselines.load_env_records(tsv)
            self.assertEqual(len(records), 1)
            self.assertEqual(records[0]["git_commit"], "abc")


if __name__ == "__main__":
    unittest.main()
