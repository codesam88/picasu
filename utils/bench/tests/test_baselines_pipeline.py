#!/usr/bin/env python3
"""Baselines pipeline tests: report -> baselines.tsv -> baselines.md."""

import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))

import battery  # noqa: E402
import render_baselines  # noqa: E402


def median(cell, **values):
    row = {"cell": cell, "commit": "abc1234", "level": cell.split("-", 1)[0]}
    row.update(values)
    return row


class BaselineMergeTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / "baselines.tsv"

    def write(self, rows):
        self.path.write_text(battery.BASELINE_HEADER + "\n" + "\n".join(rows) + "\n")


class ConfidenceMergeTest(unittest.TestCase):
    """CI fields join confidence rows onto baselined metrics by cell+metric."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / "baselines.tsv"
        self.path.write_text(battery.BASELINE_HEADER + "\n")

    @staticmethod
    def rows(text):
        parsed = {}
        for line in text.splitlines():
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            parts = line.split("\t")
            if parts[0] == "cell":
                continue
            parsed[(parts[0], parts[1])] = dict(zip(battery.BASELINE_COLUMNS, parts, strict=True))
        return parsed

    @staticmethod
    def confidence(cell, metric, low=8.0, high=10.0, samples=30, clusters=4):
        return {
            "cell": cell,
            "metric": metric,
            "median": 9.0,
            "ci_low": low,
            "ci_high": high,
            "ci_width": high - low if isinstance(low, (int, float)) else battery.NA,
            "n_samples": samples,
            "n_clusters": clusters,
            "method": "cluster-bootstrap",
        }

    def render(self, medians, confidence_rows=()):
        return battery.render_baselines_tsv(
            medians,
            self.path,
            branch="br",
            commit="abc1234",
            run_date="2026-01-02",
            confidence_rows=confidence_rows,
        )[0]


class ResolveParentsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def rep(self, parent, cell="gate-control", n=1):
        path = self.root / parent / f"{cell}-rep{n}"
        path.mkdir(parents=True)

    def test_direct_parent_passes_through(self):
        self.rep("run1")
        found, err = battery._resolve_parents([str(self.root / "run1")])
        self.assertIsNone(err)
        self.assertEqual(found, [self.root / "run1"])

    def test_run_root_expands_to_contained_batteries(self):
        self.rep("root/micro-control")
        self.rep("root/gate-control", cell="gate-upload", n=2)
        (self.root / "notes.txt").write_text("not a battery")
        found, err = battery._resolve_parents([str(self.root / "root")])
        self.assertIsNone(err)
        self.assertEqual(found, [self.root / "root/gate-control", self.root / "root/micro-control"])

    def test_missing_path_reports_an_error(self):
        found, err = battery._resolve_parents([str(self.root / "absent")])
        self.assertEqual(found, [])
        self.assertIn("not a battery directory", err)

    def test_empty_dir_yields_no_batteries(self):
        (self.root / "empty").mkdir()
        found, err = battery._resolve_parents([str(self.root / "empty")])
        self.assertEqual(found, [])
        self.assertEqual(err, "no batteries found")


class RenderTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def write_inputs(self):
        baselines = self.root / "baselines.tsv"
        baselines.write_text(
            battery.BASELINE_HEADER
            + "\n"
            + "micro-control-50:median\tscan_wall_s\t5.0\told\tscan\n"
        )
        report = self.root / "report.tsv"
        row = {column: battery.NA for column in battery.REPORT_COLUMNS}
        row.update(
            {
                "cell": "micro-control-50:median",
                "profile": "control",
                "level": "micro",
                "scale": "50",
                "build": "release",
                "commit": "abc1234",
                "scan_wall_s": "5.1",
                "scan_wall_s_min": "5.0",
                "scan_wall_s_max": "5.2",
                "idle_prefetch_p50_ms": "10.0",
                "idle_prefetch_max_ms": "11.0",
                "idle_thumb_p50_ms": "11.0",
                "idle_thumb_max_ms": "12.0",
                "idle_edit_p50_ms": "12.0",
                "idle_edit_max_ms": "13.0",
                "scroll_bar_p50_ms": "0.7",
                "scroll_bar_max_ms": "1.6",
                "scroll_rows_p50_ms": "0.7",
                "scroll_rows_max_ms": "2.0",
                "scroll_data_p50_ms": "1.1",
                "scroll_data_p50_ms_min": "1.0",
                "scroll_data_p50_ms_max": "1.2",
                "vm_hwm_mb": "370.0",
                "vm_hwm_mb_min": "369.0",
                "vm_hwm_mb_max": "371.0",
                "temp_db_mb": "1.1",
                "temp_db_mb_min": "1.0",
                "temp_db_mb_max": "1.2",
            }
        )
        gate = dict(row)
        gate.update(
            {
                "cell": "gate-control:median",
                "level": "gate",
                "scale": "100",
                "scan_prefetch_p50_ms": "8.0",
                "scan_prefetch_max_ms": "20.7",
                "tree_refresh_build_p50_us": "850.0",
                "tree_refresh_build_max_us": "1344.0",
                "filter_snapshot_clone_p50_us": "1069.5",
                "filter_snapshot_clone_max_us": "1602.5",
            }
        )
        battery.write_report(report, [row, gate])
        return baselines, report

    def write_interval_inputs(self):
        baselines, report = self.write_inputs()
        baselines.write_text(
            battery.BASELINE_HEADER
            + "\n"
            + "micro-control-50:median\tscan_wall_s\t5.0\t4.8\t5.2\t30\t4\told\tscan\n"
            + "micro-control-50:median\tscan_prefetch_p50_ms\t10.0\tn/a\tn/a\tn/a\tn/a\told\tprobe\n"
            + "gate-control:median\tscan_wall_s\t9.0\t8.5\t9.5\t40\t5\told\tgate\n"
        )
        return baselines, report

    def write_stage_interval_inputs(self):
        baselines, report = self.write_inputs()
        baselines.write_text(
            battery.BASELINE_HEADER
            + "\n"
            + "micro-control-50:median\tscan_wall_s\t5.0\t4.8\t5.2\t30\t4\told\tscan\n"
            + "gate-control:median\tscan_wall_s\t9.0\t8.5\t9.5\t40\t5\told\tgate\n"
            + "gate-control:median\ttree_refresh_build_p50_us\t850.0\t800.0\t900.0\t4\t2\told\tstage\n"
            + "gate-control:median\tfilter_snapshot_clone_p50_us\t1069.5\t1000.0\t1100.0\t4\t2\told\tstage\n"
        )
        return baselines, report

    def render_with_intervals(self, baselines, report):
        return render_baselines.render(
            render_baselines.load_reports([report]),
            render_baselines.load_baseline_values(baselines),
            intervals=render_baselines.load_baseline_confidence(baselines),
        )

    @staticmethod
    def table(text, heading):
        """Rows of one Markdown table, excluding its separator line."""
        lines = text.splitlines()
        start = lines.index(heading)
        rows = []
        for line in lines[start + 1 :]:
            if not line.startswith("|"):
                if rows:
                    break
                continue
            if line.startswith("| ---"):
                continue
            rows.append([cell.strip() for cell in line.strip("|").split("|")])
        return rows


class StormLabelTest(unittest.TestCase):
    """Render labels distinguish the named idle levels from concurrent storms."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def render_labels(self, profiles):
        report = self.root / "report.tsv"
        rows = []
        for profile in profiles:
            row = {column: battery.NA for column in battery.REPORT_COLUMNS}
            row.update(
                {
                    "cell": f"gate-{profile}:median",
                    "profile": profile,
                    "level": "gate",
                    "scale": "100",
                    "scan_storm_read_p50_ms": "3.5",
                    "scan_storm_thumb_p50_ms": "3.5",
                    "scan_storm_edit_p50_ms": "3.5",
                    "idle_storm_read_p50_ms": "1.9",
                    "idle_storm_thumb_p50_ms": "1.9",
                    "idle_storm_edit_p50_ms": "1.9",
                    "scan_storm_read_max_ms": "8.0",
                    "scan_storm_thumb_max_ms": "8.0",
                    "scan_storm_edit_max_ms": "8.0",
                    "idle_storm_read_max_ms": "4.0",
                    "idle_storm_thumb_max_ms": "4.0",
                    "idle_storm_edit_max_ms": "4.0",
                }
            )
            rows.append(row)
        battery.write_report(report, rows)
        text = render_baselines.render(render_baselines.load_reports([report]), {})
        lines = text.splitlines()
        start = lines.index("## Storm access latency")
        labels = []
        for line in lines[start + 1 :]:
            if not line.startswith("|"):
                if labels:
                    break
                continue
            if line.startswith("| ---"):
                continue
            cells = [cell.strip() for cell in line.strip("|").split("|")]
            labels.append((cells[1], cells[2]))  # scenario, access
        return labels[1:]  # drop the header row


class EnvMergeTest(unittest.TestCase):
    def run_record(self, commit="abc1234", started="2026-11-20T10:00:00+00:00", **over):
        record = {
            "level": "gate",
            "started_utc": started,
            "host": "h",
            "git_commit": commit,
        }
        record.update(over)
        return record

    def test_same_run_replaces_and_other_runs_carry_over(self):
        old = '# env: {"commit": "abc", "level": "gate", "started_utc": "2026-11-20T10:00:00+00:00", "host": "old"}'
        merged = battery.merge_env_lines(
            [old], [self.run_record(commit="abc", started="2026-11-20T10:00:00+00:00")]
        )
        self.assertEqual(len(merged), 1)
        self.assertIn('"host": "h"', merged[0])

    def test_distinct_runs_accumulate(self):
        merged = battery.merge_env_lines(
            [],
            [
                self.run_record(commit="abc", started="2026-11-20T10:00:00+00:00"),
                self.run_record(commit="def", started="2026-11-20T11:00:00+00:00"),
            ],
        )
        self.assertEqual(len(merged), 2)


class RenderEnvTest(unittest.TestCase):
    def base_args(self):
        return {
            "cell": "micro-control:50:median",
            "profile": "control",
            "level": "micro",
            "scale": "50",
            "build": "release",
            "commit": "abc1234",
            "scan_wall_s": "5.1",
        }

        # Environment is one battery-wide bullet list, not a per-level table.

    def test_load_env_records_reads_comment_lines(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tsv = Path(tmp) / "baselines.tsv"
            tsv.write_text(
                "# comment\n"
                '# env: {"commit": "abc", "level": "gate", "started_utc": "2026-11-20"}\n'
                "cell\tmetric\tbaseline\trecorded\tnote\n"
            )
            records = render_baselines.load_env_records(tsv)
            self.assertEqual(len(records), 1)
            self.assertEqual(records[0]["commit"], "abc")


if __name__ == "__main__":
    unittest.main()
