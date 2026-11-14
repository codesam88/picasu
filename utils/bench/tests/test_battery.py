#!/usr/bin/env python3
"""Tests for battery.py. Stdlib only: python3 -m unittest -v.

Battery definition, result-row extraction from fixture outdirs, and the
baseline check mode are exercised without launching a backend or workload.
"""

import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent


def load_battery():
    spec = importlib.util.spec_from_file_location("battery", BENCH / "battery.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def write_cell(
    outdir: Path,
    *,
    drain: bool = True,
    upload: bool = True,
    profile: str = "control",
) -> None:
    outdir.mkdir(parents=True, exist_ok=True)
    log = [
        "=== seeding images (repo=set-100 path=/tmp/sets/set-100)",
        "image_count=100 image_bytes=57000000",
        "scan wall median_ms=12000.0 over 2 run(s): 11000,13000",
        "scan state=completed polls=20 wall_ms=12000",
        "probe round=1 rows=100 state=completed fetch: thumb: n=100 mean=0.006 "
        "hyperfine: prefetch mean=0.020 s median=0.018 s | "
        "edit-rating mean=0.030 s median=0.028 s",
        "hyperfine compressed-get: mean=0.008 s median=0.007 s",
        "hyperfine prefetch: mean=0.015 s median=0.014 s",
        "hyperfine edit-rating: mean=0.040 s median=0.039 s",
    ]
    if drain:
        log += [
            "observe drain: scan job=1",
            "stage n p50_us p95_us max_us",
            "tree_refresh_build 100 1200 1500 2000",
            "filter_snapshot_clone 100 800 900 1000",
            "",
        ]
    if upload:
        log += [
            "upload file=u1.jpg submit_ms=10 visible_ms=50",
            "upload file=u2.jpg submit_ms=14 visible_ms=60",
            "upload submit_p50=12 visible_p50=55",
        ]
    (outdir / "harness-a.log").write_text("\n".join(log) + "\n")
    (outdir / "status-final.json").write_text(
        json.dumps(
            {
                "state": "completed",
                "scanned": 100,
                "matched": 100,
                "processed": 100,
                "failed": 0,
            }
        )
    )
    (outdir / "run-manifest.json").write_text(
        json.dumps({"build_profile": "release", "git": "abc1234", "mode": "bench"})
    )
    (outdir / "scan-walls.tsv").write_text("1\t11000\n2\t13000\n")


class BatteryDefinitionTest(unittest.TestCase):
    def setUp(self):
        self.battery = load_battery()

    def test_gate_battery_is_pinned_and_ordered(self):
        cells = self.battery.battery_cells("gate")
        self.assertEqual(
            [cell.profile for cell in cells],
            ["control", "storm-read", "storm-thumb", "storm-diff", "storm-idle", "upload"],
        )
        for cell in cells:
            self.assertEqual(cell.level, "gate")
            self.assertEqual(cell.images, 100)
            self.assertEqual(cell.repo, "set-100")
            self.assertEqual(cell.ncpu, 8)
            self.assertEqual(cell.env["SCAN_REPEATS"], "2")
            self.assertEqual(cell.env["REPEATS"], "3")
            self.assertEqual(cell.env["WARMUP"], "1")
            self.assertEqual(cell.env["BUILD_PROFILE"], "release")
            self.assertEqual(cell.env["WORKLOAD"], cell.profile)
            self.assertEqual(cell.env["IMAGES"], "100")
            self.assertEqual(cell.env["NCPU"], "8")
        self.assertEqual(
            [cell.outdir_suffix for cell in cells],
            [f"gate-{cell.profile}" for cell in cells],
        )

    def test_scale_battery_is_pinned_and_ordered(self):
        cells = self.battery.battery_cells("scale")
        self.assertEqual([cell.profile for cell in cells], ["control", "storm-diff", "upload"])
        for cell in cells:
            self.assertEqual(cell.level, "scale")
            self.assertEqual(cell.images, 5650)
            self.assertEqual(cell.repo, "set-5650")
            self.assertEqual(cell.ncpu, 8)
            self.assertEqual(cell.env["SCAN_REPEATS"], "2")

    def test_unknown_level_is_rejected(self):
        with self.assertRaises(KeyError):
            self.battery.battery_cells("nope")

    def test_cell_ids_are_unique(self):
        ids = [
            cell.cell_id
            for level in ("gate", "scale")
            for cell in self.battery.battery_cells(level)
        ]
        self.assertEqual(len(ids), len(set(ids)))


class BatteryRowTest(unittest.TestCase):
    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_row_carries_request_scan_upload_and_stage_metrics(self):
        outdir = self.root / "gate-control"
        write_cell(outdir)

        row = self.battery.build_row("gate", "control", outdir)

        self.assertEqual(row["cell"], "gate-control")
        self.assertEqual(row["profile"], "control")
        self.assertEqual(row["level"], "gate")
        self.assertEqual(row["scale"], "100")
        self.assertEqual(row["build"], "release")
        self.assertEqual(row["commit"], "abc1234")
        self.assertEqual(row["scan_wall_s"], "12.0")
        self.assertEqual(row["scan_walls_s"], "11.0,13.0")
        self.assertEqual(row["scan_fetch_p50_ms"], "18.0")
        self.assertEqual(row["fetch_worst_ms"], "18.0")
        self.assertEqual(row["scan_thumb_p50_ms"], "6.0")
        self.assertEqual(row["thumb_worst_ms"], "6.0")
        self.assertEqual(row["scan_edit_p50_ms"], "28.0")
        self.assertEqual(row["edit_worst_ms"], "28.0")
        self.assertEqual(row["idle_thumb_ms"], "7.0")
        self.assertEqual(row["idle_fetch_ms"], "14.0")
        self.assertEqual(row["idle_edit_ms"], "39.0")
        self.assertEqual(row["upload_submit_p50_ms"], "12")
        self.assertEqual(row["upload_visible_p50_ms"], "55")
        self.assertEqual(row["tree_refresh_build_p50_us"], "1200")
        self.assertEqual(row["tree_refresh_build_max_us"], "2000")
        self.assertEqual(row["filter_snapshot_clone_p50_us"], "800")
        self.assertEqual(row["filter_snapshot_clone_max_us"], "1000")

    def test_missing_drain_and_upload_columns_are_na(self):
        outdir = self.root / "gate-storm-read"
        write_cell(outdir, drain=False, upload=False, profile="storm-read")

        row = self.battery.build_row("gate", "storm-read", outdir)

        for column in (
            "upload_submit_p50_ms",
            "upload_visible_p50_ms",
            "tree_refresh_build_p50_us",
            "tree_refresh_build_max_us",
            "filter_snapshot_clone_p50_us",
            "filter_snapshot_clone_max_us",
        ):
            self.assertEqual(row[column], self.battery.NA, column)

    def test_report_has_one_row_per_cell(self):
        cells = self.battery.battery_cells("gate")
        rows = []
        for cell in cells:
            outdir = self.root / cell.outdir_suffix
            write_cell(outdir, upload=cell.profile == "upload", profile=cell.profile)
            rows.append(self.battery.build_row(cell.level, cell.profile, outdir))

        report = self.root / "report.tsv"
        self.battery.write_report(report, rows)
        loaded = self.battery.load_report(report)

        self.assertEqual(len(loaded), len(cells))
        self.assertEqual([row["cell"] for row in loaded], [cell.cell_id for cell in cells])
        self.assertEqual(list(loaded[0].keys()), self.battery.REPORT_COLUMNS)


class BaselineCheckTest(unittest.TestCase):
    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.report = self.root / "report.tsv"
        self.baselines = self.root / "baselines.tsv"

    def write_report(self, **overrides):
        row = {column: self.battery.NA for column in self.battery.REPORT_COLUMNS}
        row.update(
            {
                "cell": "gate-control",
                "profile": "control",
                "level": "gate",
                "scale": "100",
                "build": "release",
                "commit": "abc1234",
                "scan_fetch_p50_ms": "18.0",
                "scan_wall_s": "12.0",
            }
        )
        row.update(overrides)
        self.battery.write_report(self.report, [row])

    def write_baselines(self, lines):
        self.baselines.write_text("\n".join(lines) + "\n")

    def test_check_passes_inside_tolerance_band(self):
        self.write_report(scan_fetch_p50_ms="20.0")
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-control\tscan_fetch_p50_ms\t18.0\t0.25\t1.0\t2026-11-14\tgate",
            ]
        )

        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )

        self.assertEqual(result.failures, [])
        self.assertEqual(result.checked, 1)

    def test_check_fails_outside_band_and_names_cell_and_metric(self):
        self.write_report(scan_fetch_p50_ms="40.0")
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-control\tscan_fetch_p50_ms\t18.0\t0.25\t1.0\t2026-11-14\tgate",
            ]
        )

        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )

        self.assertEqual(len(result.failures), 1)
        failure = result.failures[0]
        self.assertEqual(failure.cell, "gate-control")
        self.assertEqual(failure.metric, "scan_fetch_p50_ms")
        self.assertIn("gate-control", failure.message)
        self.assertIn("scan_fetch_p50_ms", failure.message)

    def test_check_fails_when_baselined_cell_is_missing(self):
        self.write_report()
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-storm-idle\tscan_fetch_p50_ms\t18.0\t0.25\t1.0\t2026-11-14\tgate",
            ]
        )

        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )

        self.assertEqual(len(result.failures), 1)
        self.assertEqual(result.failures[0].cell, "gate-storm-idle")

    def test_check_treats_na_as_outside_band(self):
        self.write_report(scan_fetch_p50_ms=self.battery.NA)
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-control\tscan_fetch_p50_ms\t18.0\t0.25\t1.0\t2026-11-14\tgate",
            ]
        )

        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )

        self.assertEqual(len(result.failures), 1)
        self.assertIn("n/a", result.failures[0].message)

    def test_checked_in_baselines_parse_and_record_no_rows_yet(self):
        self.write_report()
        baselines = self.battery.load_baselines(BENCH / "baselines.tsv")
        self.assertEqual(baselines, [])
        result = self.battery.check_report(self.battery.load_report(self.report), baselines)
        self.assertEqual(result.failures, [])
        self.assertEqual(result.checked, 0)

    def test_cli_exit_code_follows_band_result(self):
        self.write_report(scan_fetch_p50_ms="40.0")
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-control\tscan_fetch_p50_ms\t18.0\t0.25\t1.0\t2026-11-14\tgate",
            ]
        )
        with contextlib.redirect_stdout(io.StringIO()):
            rc = self.battery.main(["check", str(self.report), "--baselines", str(self.baselines)])
        self.assertEqual(rc, 1)

        self.write_report(scan_fetch_p50_ms="18.0")
        with contextlib.redirect_stdout(io.StringIO()):
            rc = self.battery.main(["check", str(self.report), "--baselines", str(self.baselines)])
        self.assertEqual(rc, 0)


if __name__ == "__main__":
    unittest.main()
