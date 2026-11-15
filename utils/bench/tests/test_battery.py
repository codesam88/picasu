#!/usr/bin/env python3
"""Tests for battery.py. Stdlib only: python3 -m unittest -v.

Battery definition, result-row extraction from fixture outdirs, and the
baseline check mode are exercised without launching a backend or workload.
"""

import contextlib
import importlib.util
import io
import json
import os
import statistics
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

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
    walls: tuple = (11000, 13000),
) -> None:
    outdir.mkdir(parents=True, exist_ok=True)
    median_ms = statistics.median(walls)
    log = [
        "=== seeding images (repo=set-100 path=/tmp/sets/set-100)",
        "image_count=100 image_bytes=57000000",
        f"scan wall median_ms={median_ms:g} over {len(walls)} run(s): "
        + ",".join(str(wall) for wall in walls),
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
    (outdir / "scan-walls.tsv").write_text(
        "".join(f"{index}\t{wall}\n" for index, wall in enumerate(walls, 1))
    )


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


class BatteryWorkloadTimeoutTest(unittest.TestCase):
    """`run_battery` pins the per-level workload budget for run-cell.

    `battery run` invokes run-cell in-process, so no outer `timeout(1)`
    bounds a cell the way the justfile recipe does; the budget rides in
    as `WORKLOAD_TIMEOUT_S` (env-overridable).
    """

    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def cell_envs(self, level, *, preset=None):
        captured = []

        class FakeOrchestrator:
            def config_from_env(self, env):
                return env

            def run_cell(self, cfg, env):
                captured.append(env)
                return 0

        row = {column: self.battery.NA for column in self.battery.REPORT_COLUMNS}
        with (
            mock.patch.dict(os.environ, {}, clear=False),
            mock.patch.object(self.battery, "_run_cell", return_value=FakeOrchestrator()),
            mock.patch.object(self.battery, "build_row", return_value=row),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            os.environ.pop("WORKLOAD_TIMEOUT_S", None)
            os.environ["BATTERY_REPEATS"] = "1"
            if preset is not None:
                os.environ["WORKLOAD_TIMEOUT_S"] = preset
            rc = self.battery.run_battery(level, self.root / level)
        self.assertEqual(rc, 0)
        return captured

    def test_gate_cells_get_an_1800s_workload_budget(self):
        envs = self.cell_envs("gate")
        self.assertEqual(len(envs), len(self.battery.battery_cells("gate")))
        self.assertTrue(all(env["WORKLOAD_TIMEOUT_S"] == "1800" for env in envs))

    def test_scale_cells_get_a_7200s_workload_budget(self):
        envs = self.cell_envs("scale")
        self.assertEqual(len(envs), len(self.battery.battery_cells("scale")))
        self.assertTrue(all(env["WORKLOAD_TIMEOUT_S"] == "7200" for env in envs))

    def test_exported_workload_timeout_wins_over_the_level_default(self):
        envs = self.cell_envs("gate", preset="999")
        self.assertTrue(all(env["WORKLOAD_TIMEOUT_S"] == "999" for env in envs))


if __name__ == "__main__":
    unittest.main()


class BatteryExpansionTest(unittest.TestCase):
    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def run_with_repeats(self, repeats):
        captured = []

        class FakeOrchestrator:
            def config_from_env(self, env):
                return env

            def run_cell(self, cfg, env):
                captured.append(env["OUT"])
                return 0

        row = {column: self.battery.NA for column in self.battery.REPORT_COLUMNS}
        row.update({"cell": "gate-control", "profile": "control", "level": "gate"})
        with (
            mock.patch.dict(os.environ, {}, clear=False),
            mock.patch.object(self.battery, "_run_cell", return_value=FakeOrchestrator()),
            mock.patch.object(self.battery, "build_row", return_value=dict(row)),
            mock.patch.object(
                self.battery, "_build_median_row", return_value={"cell": "gate-control:median"}
            ),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            os.environ.pop("WORKLOAD_TIMEOUT_S", None)
            os.environ["BATTERY_REPEATS"] = str(repeats)
            rc = self.battery.run_battery("gate", self.root / "gate")
        self.assertEqual(rc, 0)
        return captured, self.root / "gate" / "report.tsv"

    def test_expands_into_n_outdirs(self):
        captured, report = self.run_with_repeats(3)
        cells = self.battery.battery_cells("gate")
        self.assertEqual(len(captured), 3 * len(cells))
        for cell in cells:
            for rep in range(1, 4):
                self.assertTrue(
                    any(Path(p).name == f"{cell.outdir_suffix}-rep{rep}" for p in captured),
                    f"missing {cell.outdir_suffix}-rep{rep}",
                )
        lines = report.read_text().splitlines()
        self.assertEqual(len(lines) - 1, 4 * len(cells))
        self.assertEqual(sum(1 for line in lines if ":median" in line), len(cells))

    def test_repeats_one_is_backward_compatible(self):
        captured, report = self.run_with_repeats(1)
        cells = self.battery.battery_cells("gate")
        self.assertEqual(len(captured), len(cells))
        lines = report.read_text().splitlines()
        self.assertEqual(len(lines) - 1, len(cells))
        self.assertEqual(sum(1 for line in lines if ":median" in line), 0)
        for cell in cells:
            self.assertTrue(any(Path(p).name == f"{cell.outdir_suffix}-rep1" for p in captured))


class MedianRowTest(unittest.TestCase):
    def setUp(self):
        self.battery = load_battery()

    def test_median_math_odd_even_and_na(self):
        row_a = {
            "cell": "gate-control",
            "profile": "control",
            "level": "gate",
            "scale": "100",
            "build": "release",
            "commit": "abc",
            "scan_wall_s": "10.0",
            "scan_fetch_p50_ms": self.battery.NA,
            "idle_thumb_ms": self.battery.NA,
        }
        row_b = {
            "cell": "gate-control",
            "profile": "control",
            "level": "gate",
            "scale": "100",
            "build": "release",
            "commit": "abc",
            "scan_wall_s": "20.0",
            "scan_fetch_p50_ms": "18.0",
            "idle_thumb_ms": self.battery.NA,
        }
        row_c = {
            "cell": "gate-control",
            "profile": "control",
            "level": "gate",
            "scale": "100",
            "build": "release",
            "commit": "abc",
            "scan_wall_s": "30.0",
            "scan_fetch_p50_ms": "22.0",
            "idle_thumb_ms": self.battery.NA,
        }
        odd = self.battery._build_median_row([row_a, row_b, row_c])
        self.assertEqual(odd["cell"], "gate-control:median")
        self.assertEqual(odd["scan_wall_s"], "20.0")
        self.assertEqual(odd["scan_fetch_p50_ms"], "20.0")
        self.assertEqual(odd["idle_thumb_ms"], self.battery.NA)

        even = self.battery._build_median_row([row_a, row_b])
        self.assertEqual(even["scan_wall_s"], "15.0")

    def test_non_numeric_columns_pass_through_from_first_repeat(self):
        row_a = {
            "cell": "gate-control",
            "profile": "first",
            "level": "gate",
            "scale": "100",
            "build": "release",
            "commit": "abc",
            "scan_wall_s": "10.0",
        }
        row_b = {
            "cell": "gate-control",
            "profile": "second",
            "level": "gate",
            "scale": "200",
            "build": "debug",
            "commit": "def",
            "scan_wall_s": "12.0",
        }
        med = self.battery._build_median_row([row_a, row_b])
        self.assertEqual(med["profile"], "first")
        self.assertEqual(med["scale"], "100")
        self.assertEqual(med["build"], "release")
        self.assertEqual(med["commit"], "abc")
        self.assertEqual(med["scan_wall_s"], "11.0")

    def test_comma_lists_are_pooled_across_repeats_not_median_of_medians(self):
        row_a = {
            "cell": "gate-control",
            "tree_refresh_build_p50_us": "0,10",
            "scan_walls_s": "11,13",
        }
        row_b = {
            "cell": "gate-control",
            "tree_refresh_build_p50_us": "10,10",
            "scan_walls_s": "9,15",
        }
        med = self.battery._build_median_row([row_a, row_b])
        # Pooled samples are [0, 10, 10, 10]; the per-repeat medians
        # (5.0, 10.0) would give 7.5.
        self.assertEqual(float(med["tree_refresh_build_p50_us"]), 10.0)
        self.assertEqual(float(med["scan_walls_s"]), 12.0)

    def test_comma_lists_skip_na_entries_and_keep_single_values(self):
        row_a = {
            "cell": "gate-control",
            "filter_snapshot_clone_p50_us": "376,1483",
            "scan_fetch_p50_ms": "18.0",
        }
        row_b = {
            "cell": "gate-control",
            "filter_snapshot_clone_p50_us": self.battery.NA,
            "scan_fetch_p50_ms": "20.0",
        }
        row_c = {
            "cell": "gate-control",
            "filter_snapshot_clone_p50_us": "425,1500",
            "scan_fetch_p50_ms": self.battery.NA,
        }
        med = self.battery._build_median_row([row_a, row_b, row_c])
        # Pools [376, 1483, 425, 1500] and [18.0, 20.0]; the n/a repeats
        # contribute nothing and the all-n/a column stays n/a.
        self.assertEqual(float(med["filter_snapshot_clone_p50_us"]), 954.0)
        self.assertEqual(float(med["scan_fetch_p50_ms"]), 19.0)
        self.assertEqual(med["upload_submit_p50_ms"], self.battery.NA)

    def test_stage_zero_reading_is_numeric_not_na(self):
        # A drain stage with no samples is logged as an explicit 0 by the
        # backend; the battery must keep it as data, not turn it into n/a.
        rows = [{"cell": "gate-storm-idle", "filter_snapshot_clone_p50_us": "0"} for _ in range(3)]
        med = self.battery._build_median_row(rows)
        self.assertEqual(float(med["filter_snapshot_clone_p50_us"]), 0.0)

    def test_pooled_median_avoids_float_representation_noise(self):
        rows = [
            {"cell": "gate-storm-diff", "scan_walls_s": "20.188,3.027"},
            {"cell": "gate-storm-diff", "scan_walls_s": "10.288,3.026"},
            {"cell": "gate-storm-diff", "scan_walls_s": "11.103,2.826"},
        ]
        med = self.battery._build_median_row(rows)
        self.assertEqual(med["scan_walls_s"], "6.6575")

    def test_median_of_built_rows_pools_scan_walls_and_stage_lists(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        repeat_walls = ((11000, 13000), (9000, 15000), (12000, 12000))
        rows = []
        for index, walls in enumerate(repeat_walls, 1):
            outdir = root / f"gate-control-rep{index}"
            write_cell(outdir, walls=walls)
            rows.append(self.battery.build_row("gate", "control", outdir))

        med = self.battery._build_median_row(rows)

        self.assertEqual(med["cell"], "gate-control:median")
        # Pooled [9, 11, 12, 12, 13, 15] s.
        self.assertEqual(med["scan_walls_s"], "12.0")
        self.assertEqual(float(med["tree_refresh_build_p50_us"]), 1200.0)
        self.assertEqual(float(med["filter_snapshot_clone_max_us"]), 1000.0)


class BatteryCheckMedianTest(unittest.TestCase):
    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.report = self.root / "report.tsv"
        self.baselines = self.root / "baselines.tsv"

    def test_failing_repeat_with_passing_median_passes(self):
        rows = [
            {
                "cell": "gate-control",
                "profile": "control",
                "level": "gate",
                "scale": "100",
                "build": "release",
                "commit": "abc",
                "scan_fetch_p50_ms": "40.0",
            },
            {
                "cell": "gate-control:median",
                "profile": "control",
                "level": "gate",
                "scale": "100",
                "build": "release",
                "commit": "abc",
                "scan_fetch_p50_ms": "18.0",
            },
        ]
        self.battery.write_report(self.report, rows)
        self.baselines.write_text(
            self.battery.BASELINE_HEADER
            + "\n"
            + "gate-control:median\tscan_fetch_p50_ms\t18.0\t0.25\t1.0\t2026-11-14\tgate\n"
        )
        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )
        self.assertEqual(result.failures, [])
        self.assertEqual(result.checked, 1)

    def test_failing_median_row_fails(self):
        rows = [
            {
                "cell": "gate-control:median",
                "profile": "control",
                "level": "gate",
                "scale": "100",
                "build": "release",
                "commit": "abc",
                "scan_fetch_p50_ms": "40.0",
            },
        ]
        self.battery.write_report(self.report, rows)
        self.baselines.write_text(
            self.battery.BASELINE_HEADER
            + "\n"
            + "gate-control:median\tscan_fetch_p50_ms\t18.0\t0.25\t1.0\t2026-11-14\tgate\n"
        )
        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )
        self.assertEqual(len(result.failures), 1)


class BatteryResumeTest(unittest.TestCase):
    """`battery run` resumes completed reps by default; `--fresh` re-runs.

    A rep is complete when its outdir carries `status-final.json` and the
    artifacts still parse through `build_row`; anything else runs again.
    The report is rebuilt from scratch on every invocation.
    """

    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.outdir = self.root / "gate"
        self.cell = self.battery.battery_cells("gate")[0]
        self.report = self.outdir / "report.tsv"

    def complete_rep(self, rep, *, walls=(11000, 13000)):
        """Write a finished-rep outdir that `build_row` parses."""
        out = self.outdir / f"{self.cell.outdir_suffix}-rep{rep}"
        write_cell(out, walls=walls)
        return out

    def run_battery(self, *, repeats=3, fresh=False, fail=(), write=True):
        """Run a one-cell battery against a counting fake orchestrator.

        `fail` names rep outdirs whose run_cell exits 7 without writing
        artifacts (an interrupted/failed rep). Returns (invoked outdirs,
        stdout, return code).
        """
        invoked = []
        stdout = io.StringIO()

        class FakeOrchestrator:
            def config_from_env(self, env):
                return env

            def run_cell(self, cfg, env):
                out = Path(env["OUT"])
                invoked.append(out)
                if out.name in fail:
                    return 7
                if write:
                    write_cell(out)
                return 0

        with (
            mock.patch.dict(os.environ, {"BATTERY_REPEATS": str(repeats)}, clear=False),
            mock.patch.object(self.battery, "battery_cells", return_value=(self.cell,)),
            mock.patch.object(self.battery, "_run_cell", return_value=FakeOrchestrator()),
            contextlib.redirect_stdout(stdout),
            contextlib.redirect_stderr(io.StringIO()),
        ):
            rc = self.battery.run_battery("gate", self.outdir, fresh=fresh)
        return invoked, stdout.getvalue(), rc

    def rep_names(self, invoked):
        return [out.name for out in invoked]

    def test_resume_reuses_completed_rep_and_runs_missing_ones(self):
        self.complete_rep(1)

        invoked, stdout, rc = self.run_battery(repeats=3)

        self.assertEqual(rc, 0)
        self.assertEqual(
            self.rep_names(invoked),
            [f"{self.cell.outdir_suffix}-rep2", f"{self.cell.outdir_suffix}-rep3"],
        )
        self.assertIn("rep=1/3 reused", stdout)
        rows = self.battery.load_report(self.report)
        self.assertEqual(
            [row["cell"] for row in rows],
            ["gate-control", "gate-control", "gate-control", "gate-control:median"],
        )

    def test_corrupt_status_final_reruns_the_rep(self):
        rep1 = self.complete_rep(1)
        (rep1 / "status-final.json").write_text('{"state": ')
        rep2 = self.complete_rep(2)
        (rep2 / "harness-a.log").unlink()

        invoked, stdout, rc = self.run_battery(repeats=2)

        self.assertEqual(rc, 0)
        self.assertEqual(
            self.rep_names(invoked),
            [f"{self.cell.outdir_suffix}-rep1", f"{self.cell.outdir_suffix}-rep2"],
        )
        self.assertNotIn("reused", stdout)

    def test_fresh_flag_reruns_every_rep(self):
        self.complete_rep(1)
        self.complete_rep(2)

        invoked, stdout, rc = self.run_battery(repeats=2, fresh=True)

        self.assertEqual(rc, 0)
        self.assertEqual(
            self.rep_names(invoked),
            [f"{self.cell.outdir_suffix}-rep1", f"{self.cell.outdir_suffix}-rep2"],
        )
        self.assertNotIn("reused", stdout)
        self.assertEqual(len(self.battery.load_report(self.report)), 3)

    def test_fully_resumed_cell_still_gets_a_median_row(self):
        self.complete_rep(1, walls=(11000, 13000))
        self.complete_rep(2, walls=(9000, 15000))
        self.complete_rep(3, walls=(12000, 12000))

        invoked, stdout, rc = self.run_battery(repeats=3)

        self.assertEqual(rc, 0)
        self.assertEqual(invoked, [])
        self.assertEqual(stdout.count("reused"), 3)
        rows = self.battery.load_report(self.report)
        self.assertEqual(len(rows), 4)
        median = rows[-1]
        self.assertEqual(median["cell"], "gate-control:median")
        # Pooled [9, 11, 12, 12, 13, 15] s across the three repeats.
        self.assertEqual(median["scan_walls_s"], "12.0")
        self.assertEqual(median["tree_refresh_build_p50_us"], "1200.0")

    def test_retry_after_failure_resumes_without_duplicate_rows(self):
        failed = f"{self.cell.outdir_suffix}-rep3"

        invoked, _stdout, rc = self.run_battery(repeats=3, fail=(failed,))
        self.assertEqual(rc, 7)  # fail-fast keeps the cell's exit code
        self.assertEqual(len(invoked), 3)
        self.assertEqual(len(self.battery.load_report(self.report)), 2)

        invoked, stdout, rc = self.run_battery(repeats=3)

        self.assertEqual(rc, 0)
        self.assertEqual(self.rep_names(invoked), [failed])
        self.assertEqual(stdout.count("reused"), 2)
        rows = self.battery.load_report(self.report)
        self.assertEqual(
            [row["cell"] for row in rows],
            ["gate-control"] * 3 + ["gate-control:median"],
        )

    def test_cli_fresh_flag_reaches_run_battery(self):
        with (
            mock.patch.object(self.battery, "run_battery", return_value=0) as run,
            contextlib.redirect_stdout(io.StringIO()),
        ):
            rc = self.battery.main(["run", "gate", "--out", str(self.outdir), "--fresh"])
        self.assertEqual(rc, 0)
        self.assertIs(run.call_args.kwargs["fresh"], True)

        with (
            mock.patch.object(self.battery, "run_battery", return_value=0) as run,
            contextlib.redirect_stdout(io.StringIO()),
        ):
            rc = self.battery.main(["run", "gate", "--out", str(self.outdir)])
        self.assertEqual(rc, 0)
        self.assertIs(run.call_args.kwargs["fresh"], False)
