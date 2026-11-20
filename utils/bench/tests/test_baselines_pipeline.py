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

    def test_existing_note_is_kept_and_baseline_moves(self):
        self.write(["gate-control:median\tscan_wall_s\t9.0\told\twhy"])
        text, new = battery.render_baselines_tsv(
            [median("gate-control:median", scan_wall_s="9.5")],
            self.path,
            branch="br",
            commit="abc1234",
            run_date="2026-01-02",
        )
        row = text.splitlines()[-1].split("\t")
        self.assertEqual(new, [])
        self.assertEqual(
            row, ["gate-control:median", "scan_wall_s", "9.5", "2026-01-02 br abc1234", "why"]
        )

    def test_new_metric_gets_empty_note(self):
        self.write(["gate-control:median\tscan_wall_s\t9.0\told\twhy"])
        text, new = battery.render_baselines_tsv(
            [median("gate-control:median", scan_wall_s="9.5", scan_edit_max_ms="20.0")],
            self.path,
            branch="br",
            commit="abc1234",
            run_date="2026-01-02",
        )
        self.assertEqual(new, [("gate-control:median", "scan_edit_max_ms")])
        self.assertIn(
            "gate-control:median\tscan_edit_max_ms\t20.0\t2026-01-02 br abc1234\tnew metric recorded 2026-01-02\n",
            text,
        )

    def test_cells_absent_from_the_battery_are_preserved(self):
        self.write(
            [
                "micro-control-50:median\tscan_wall_s\t5.0\told\tkept",
                "gate-control:median\tscan_wall_s\t9.0\told\twhy",
            ]
        )
        text, _ = battery.render_baselines_tsv(
            [median("gate-control:median", scan_wall_s="9.5")],
            self.path,
            branch="br",
            commit="abc1234",
            run_date="2026-01-02",
        )
        self.assertIn("micro-control-50:median\tscan_wall_s\t5.0\told\tkept", text)


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

    def test_render_owns_the_document_and_defines_terms_once(self):
        baselines, report = self.write_inputs()
        text = render_baselines.render(
            render_baselines.load_reports([report]),
            render_baselines.load_baseline_values(baselines),
        )
        self.assertIn("# Benchmark baselines", text)
        self.assertNotIn("GENERATED", text)
        self.assertNotIn("## Metric reference", text)
        # Sections are scenarios plus cost and footprint, not batteries.
        self.assertIn("## Memory footprint", text)
        self.assertIn("## Idle access latency", text)
        self.assertIn("## Scan access latency", text)
        self.assertIn("## Scan wall time and stages", text)
        self.assertLess(text.index("## Memory footprint"), text.index("## Idle access latency"))
        self.assertNotIn("## Micro battery", text)
        self.assertNotIn("## Gate battery", text)
        self.assertNotIn("## Upload access latency", text)
        self.assertNotIn("## Burst access latency", text)
        # Terms are defined once up front, one definition-style bullet each.
        self.assertIn("## Terms", text)
        self.assertIn("- **#images**: number of images in the scanned set.", text)
        self.assertIn("- **worst_case**: worst observed repeat value.", text)
        self.assertIn("- **wall time**:", text)
        self.assertIn("- **peak memory**:", text)
        self.assertIn("- **units**:", text)
        self.assertNotIn("p50", text)
        # Rows are accesses keyed by image count.
        self.assertIn("| #images | access", text)
        self.assertIn("| read", text)
        self.assertIn("| scroll data", text)
        self.assertNotIn("| cell", text)
        self.assertNotIn("### Resources", text)
        # The recorded baseline wins for a baselined metric; the report fills the rest.
        self.assertIn("5.0", text)
        self.assertIn("370.0", text)
        self.assertIn("11.0", text)

    def test_cli_writes_the_target(self):
        baselines, report = self.write_inputs()
        out = self.root / "baselines.md"
        rc = render_baselines.main(
            [
                str(report),
                "--baselines",
                str(baselines),
                "--out",
                str(out),
            ]
        )
        self.assertEqual(rc, 0)
        self.assertIn("## Idle access latency", out.read_text())


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

    def test_run_environment_section_lists_runs(self):
        report = {"micro-control:50:median": dict(self.base_args())}
        baselines = {
            ("micro-control-50:median", "scan_wall_s"): "5.0",
        }
        runs = [
            {
                "level": "micro",
                "started_utc": "2026-11-20T10:00:00+00:00",
                "host": "benchbox",
                "cpu_model": "Fake CPU",
                "cpu_count": 8,
                "freq_cur_mhz": 2400,
                "freq_max_mhz": 2400,
                "governor": "powersave",
                "mem_total_kb": 65804280,
                "load_1": 1.5,
                "load_5": 1.2,
                "load_15": 0.9,
                "git_commit": "abc1234",
            }
        ]
        text = render_baselines.render(report, baselines, runs)
        self.assertIn("## Run environment", text)
        self.assertIn("- **host**: benchbox", text)
        self.assertIn("- **CPU**: Fake CPU, 8 cores, 2400/2400 MHz", text)
        self.assertIn("abc1234"[:12], text)
        # Environment is one battery-wide bullet list, not a per-level table.
        self.assertNotIn("| level |", text)

    def test_no_env_runs_means_no_section(self):
        report = {"micro-control-50:median": dict(self.base_args())}
        baselines = {("micro-control-50:median", "scan_wall_s"): "5.0"}
        text = render_baselines.render(report, baselines, [])
        self.assertNotIn("## Run environment", text)

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
