#!/usr/bin/env python3
"""Tests for battery.py. Stdlib only: python3 -m unittest -v.

Battery definition, result-row extraction from fixture outdirs, and the
baseline check mode are exercised without launching a backend or workload.
"""

import contextlib
import datetime
import io
import json
import os
import shutil
import statistics
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))
FIXTURES = Path(__file__).resolve().parent / "fixtures"


def load_battery():
    import battery

    return battery


def write_cell(
    outdir: Path,
    *,
    drain: bool = True,
    upload: bool = True,
    profile: str = "control",
    walls: tuple = (11000, 13000),
    probe: tuple | None = (0.018, 0.006, 0.028),
    post: tuple = (0.014, 0.007, 0.039),
    storm: tuple | None = None,
) -> None:
    """Write the harness artifacts one finished rep leaves behind.

    All timings are seconds. `probe` is the during-scan probe medians
    (prefetch, thumb fetch, edit-rating); `None` omits the probe rounds,
    as the storm profiles do. `post` is the post-scan hyperfine medians
    (prefetch, thumb, edit) that fill the idle background. `storm` is
    `(profile, median, p95, max)`; the per-op TSV is synthesized so its
    aggregate reproduces exactly those summary values.
    """
    outdir.mkdir(parents=True, exist_ok=True)
    median_ms = statistics.median(walls)
    post_pref, post_thumb, post_edit = post
    log = [
        "=== seeding images (repo=set-100 path=/tmp/sets/set-100)",
        "image_count=100 image_bytes=57000000",
        f"scan wall median_ms={median_ms:g} over {len(walls)} run(s): "
        + ",".join(str(wall) for wall in walls),
        "scan state=completed polls=20 wall_ms=12000",
    ]
    if probe is not None:
        probe_pref, probe_thumb, probe_edit = probe
        log.append("probe round=1 rows=100 state=completed")
        rounds_dir = outdir / "rounds-run-1"
        rounds_dir.mkdir(exist_ok=True)
        (rounds_dir / "hyperfine-round-1.json").write_text(
            json.dumps(
                {
                    "results": [
                        {"name": "prefetch", "mean": probe_pref, "median": probe_pref},
                        {"name": "edit-rating", "mean": probe_edit, "median": probe_edit},
                    ]
                }
            )
            + "\n"
        )
        (rounds_dir / "probe-round-1.tsv").write_text(
            "".join(f"h{i:04d}\t200\t{probe_thumb}\n" for i in range(100))
        )
    log.append("=== hyperfine (warmup=1 runs=3) ===")
    (outdir / "hyperfine.json").write_text(
        json.dumps(
            {
                "results": [
                    {"name": "compressed-get", "median": post_thumb},
                    {"name": "prefetch", "median": post_pref},
                    {"name": "edit-rating", "median": post_edit},
                ]
            }
        )
        + "\n"
    )
    if storm is not None:
        storm_profile, storm_median, storm_p95, storm_max = storm
        log.append(f"storm: profile={storm_profile} ops=80/80 failed=0 wall=2.0s")
        lo = storm_median / 2
        mid = (storm_median + storm_p95) / 2
        assert lo < storm_median < mid < storm_p95 <= storm_max
        latencies = [lo] * 10 + [storm_median] + [mid] * 8 + [storm_p95, storm_max]
        rounds_dir = outdir / "rounds-run-1"
        rounds_dir.mkdir(exist_ok=True)
        (rounds_dir / "storm-round-1.tsv").write_text(
            "".join(f"{i}\t0\t{lat}\t200\n" for i, lat in enumerate(latencies))
        )
    if drain:
        # Stage costs come from the backend's server.log, never from the
        # transcript. Copy the recorded drain record so the tests read the
        # artifact a real cell writes.
        shutil.copyfile(FIXTURES / "server-drain.log", outdir / "server.log")
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

    def test_micro_battery_is_pinned_and_ordered(self):
        cells = self.battery.battery_cells("micro")
        self.assertEqual([cell.profile for cell in cells], ["control", "control"])
        self.assertEqual([cell.images for cell in cells], [50, 200])
        self.assertEqual([cell.repo for cell in cells], ["set-50", "set-200"])
        for cell in cells:
            self.assertEqual(cell.level, "micro")
            self.assertEqual(cell.ncpu, 8)
            self.assertEqual(cell.env["SCAN_REPEATS"], "2")
            self.assertEqual(cell.env["REPEATS"], "3")
            self.assertEqual(cell.env["WARMUP"], "1")
            self.assertEqual(cell.env["BUILD_PROFILE"], "release")
            self.assertEqual(cell.env["WORKLOAD"], cell.profile)
            self.assertEqual(cell.env["NCPU"], "8")
        self.assertEqual(
            [cell.outdir_suffix for cell in cells],
            ["micro-control-50", "micro-control-200"],
        )

    def test_unknown_level_is_rejected(self):
        with self.assertRaises(KeyError):
            self.battery.battery_cells("nope")


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
        # During-scan probes on a control cell land in the `scan` background.
        self.assertEqual(row["scan_prefetch_p50_ms"], "18.0")
        self.assertEqual(row["scan_prefetch_max_ms"], "18.0")
        self.assertEqual(row["scan_thumb_p50_ms"], "6.0")
        self.assertEqual(row["scan_thumb_max_ms"], "6.0")
        self.assertEqual(row["scan_edit_p50_ms"], "28.0")
        self.assertEqual(row["scan_edit_max_ms"], "28.0")
        # The upload-profile probes are absent from a control row, and the
        # storm backgrounds did not run.
        for column in (
            "uscan_prefetch_p50_ms",
            "uscan_thumb_p50_ms",
            "uscan_edit_p50_ms",
            "scan_storm_read_p50_ms",
            "scan_storm_edit_p50_ms",
            "idle_storm_read_p50_ms",
            "scan_storm_edit_failed",
            "idle_storm_read_failed",
        ):
            self.assertEqual(row[column], self.battery.NA, column)
        self.assertEqual(row["idle_thumb_p50_ms"], "7.0")
        self.assertEqual(row["idle_prefetch_p50_ms"], "14.0")
        self.assertEqual(row["idle_edit_p50_ms"], "39.0")
        self.assertEqual(row["upload_submit_p50_ms"], "12")
        self.assertEqual(row["upload_visible_p50_ms"], "55")
        # Stage columns are the recorded table's two scans, in order.
        self.assertEqual(row["tree_refresh_build_p50_us"], "542,1002")
        self.assertEqual(row["tree_refresh_build_max_us"], "1997,6069")
        self.assertEqual(row["filter_snapshot_clone_p50_us"], "419,1493")
        self.assertEqual(row["filter_snapshot_clone_max_us"], "1778,5236")

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


class ReportSchemaTest(unittest.TestCase):
    """The report is the extractor's full background x access x stat matrix."""

    # The ad-hoc columns this schema replaces. Each carried exactly one
    # number of exactly one matrix cell, so the migration is 1:1.
    LEGACY_METRICS = {
        "scan_fetch_p50_ms": "scan_prefetch_p50_ms",
        "fetch_worst_ms": "scan_prefetch_max_ms",
        "scan_thumb_p50_ms": "scan_thumb_p50_ms",
        "thumb_worst_ms": "scan_thumb_max_ms",
        "scan_edit_p50_ms": "scan_edit_p50_ms",
        "edit_worst_ms": "scan_edit_max_ms",
        "idle_thumb_ms": "idle_thumb_p50_ms",
        "idle_fetch_ms": "idle_prefetch_p50_ms",
        "idle_edit_ms": "idle_edit_p50_ms",
    }

    def setUp(self):
        self.battery = load_battery()

    def test_matrix_has_every_background_access_stat_combination(self):
        columns = self.battery.REPORT_COLUMNS
        latency = [column for column in columns if column.endswith("_ms") and "storm" in column]
        self.assertEqual(len(latency), 27)  # 3 backgrounds x 3 storm kinds x 3 stats
        for background in ("idle", "scan", "uscan"):
            for access in ("prefetch", "thumb", "edit"):
                for stat in ("p50", "p95", "max"):
                    self.assertIn(f"{background}_{access}_{stat}_ms", columns)
        for background in ("idle", "scan"):
            for kind in ("read", "thumb", "edit"):
                self.assertIn(f"{background}_storm_{kind}_failed", columns)
        self.assertNotIn("uscan_storm_read_failed", columns)
        # 3 backgrounds x 6 accesses x 3 stats, storm accesses included,
        # plus the two upload submit/visible p50 columns.
        self.assertEqual(sum(1 for column in columns if column.endswith("_ms")), 65)

    def test_checked_in_baselines_use_only_the_new_names(self):
        metrics = {
            baseline.metric for baseline in self.battery.load_baselines(BENCH / "baselines.tsv")
        }
        for legacy, replacement in self.LEGACY_METRICS.items():
            if legacy != replacement:
                self.assertNotIn(legacy, metrics)
        for metric in metrics:
            self.assertIn(metric, self.battery.REPORT_COLUMNS, metric)


class BaselineCheckTest(unittest.TestCase):
    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.report = self.root / "report.tsv"
        self.baselines_tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.baselines_tmp.cleanup)
        self.baselines = Path(self.baselines_tmp.name) / "baselines.tsv"

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
                "scan_prefetch_p50_ms": "18.0",
                "scan_wall_s": "12.0",
            }
        )
        row.update(overrides)
        self.battery.write_report(self.report, [row])

    def write_baselines(self, lines):
        self.baselines.write_text("\n".join(lines) + "\n")

    def test_check_passes_inside_guardrail(self):
        self.write_report(scan_prefetch_p50_ms="20.0")
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-control\tscan_prefetch_p50_ms\t18.0\t2026-11-14\tgate",
            ]
        )

        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )

        self.assertEqual(result.failures, [])
        self.assertEqual(result.checked, 1)

    def test_check_passes_within_absolute_floor(self):
        # limit * 0.5 = 500 would fail, but the 100 ms floor covers it
        self.write_report(scan_prefetch_p50_ms="1400.0")
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-control\tscan_prefetch_p50_ms\t1000.0\t2026-11-14\tgate",
            ]
        )

        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )

        self.assertEqual(result.failures, [])

    def test_check_fails_outside_guardrail_and_names_cell_and_metric(self):
        self.write_report(scan_prefetch_p50_ms="200.0")
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-control\tscan_prefetch_p50_ms\t18.0\t2026-11-14\tgate",
            ]
        )

        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )

        self.assertEqual(len(result.failures), 1)
        failure = result.failures[0]
        self.assertEqual(failure.cell, "gate-control")
        self.assertEqual(failure.metric, "scan_prefetch_p50_ms")
        self.assertIn("gate-control", failure.message)
        self.assertIn("scan_prefetch_p50_ms", failure.message)

    def test_check_fails_when_baselined_cell_is_missing(self):
        self.write_report()
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-storm-idle\tscan_prefetch_p50_ms\t18.0\t2026-11-14\tgate",
            ]
        )

        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )

        self.assertEqual(len(result.failures), 1)
        self.assertEqual(result.failures[0].cell, "gate-storm-idle")

    def test_check_treats_na_as_failure(self):
        self.write_report(scan_prefetch_p50_ms=self.battery.NA)
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-control\tscan_prefetch_p50_ms\t18.0\t2026-11-14\tgate",
            ]
        )

        result = self.battery.check_report(
            self.battery.load_report(self.report),
            self.battery.load_baselines(self.baselines),
        )

        self.assertEqual(len(result.failures), 1)
        self.assertIn("n/a", result.failures[0].message)

    def test_checked_in_baselines_parse(self):
        baselines = self.battery.load_baselines(BENCH / "baselines.tsv")
        self.assertGreater(len(baselines), 0)
        for baseline in baselines:
            self.assertTrue(baseline.cell.endswith(":median"))
            self.assertTrue(baseline.metric)
            self.assertGreaterEqual(baseline.baseline, 0.0)

    def test_cli_exit_code_follows_guardrail_result(self):
        self.write_report(scan_prefetch_p50_ms="200.0")
        self.write_baselines(
            [
                self.battery.BASELINE_HEADER,
                "gate-control\tscan_prefetch_p50_ms\t18.0\t2026-11-14\tgate",
            ]
        )
        with contextlib.redirect_stdout(io.StringIO()):
            rc = self.battery.main(["check", str(self.report), "--baselines", str(self.baselines)])
        self.assertEqual(rc, 1)

        self.write_report(scan_prefetch_p50_ms="18.0")
        with contextlib.redirect_stdout(io.StringIO()):
            rc = self.battery.main(["check", str(self.report), "--baselines", str(self.baselines)])
        self.assertEqual(rc, 0)


class BatteryDefaultOutTest(unittest.TestCase):
    def test_run_without_out_uses_timestamped_default(self):
        import signal

        battery = load_battery()
        previous = signal.getsignal(signal.SIGTERM)
        try:
            with (
                mock.patch.dict(os.environ, {}, clear=False),
                mock.patch.object(battery, "run_battery", return_value=0) as run,
                contextlib.redirect_stdout(io.StringIO()),
            ):
                os.environ.pop("OUT", None)
                rc = battery.main(["run", "cell"])
        finally:
            signal.signal(signal.SIGTERM, previous)
        self.assertEqual(rc, 0)
        outdir = str(run.call_args[0][1])
        self.assertRegex(outdir, r"/tmp/picasu-battery-cell-\d{8}-\d{6}$")


class BatteryWorkloadTimeoutTest(unittest.TestCase):
    """`run_battery` pins the per-level workload budget for run_cell.

    `battery run` invokes run_cell in-process, so no outer `timeout(1)`
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
            mock.patch.object(self.battery, "run_cell", FakeOrchestrator()),
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

    def test_micro_cells_get_an_1800s_workload_budget(self):
        envs = self.cell_envs("micro")
        self.assertEqual(len(envs), len(self.battery.battery_cells("micro")))
        self.assertTrue(all(env["WORKLOAD_TIMEOUT_S"] == "1800" for env in envs))

    def test_exported_workload_timeout_wins_over_the_level_default(self):
        envs = self.cell_envs("gate", preset="999")
        self.assertTrue(all(env["WORKLOAD_TIMEOUT_S"] == "999" for env in envs))


class CellLevelDefinitionTest(unittest.TestCase):
    """The `cell` battery level: one fully-parameterized cell from env."""

    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()

    def test_one_cell_honors_env_overrides(self):
        environ = {
            "WORKLOAD": "upload",
            "IMAGES": "42",
            "NCPU": "2",
            "BUILD_PROFILE": "debug",
            "IMAGE_REPO": str(self.repo),
            "SCAN_REPEATS": "5",
        }
        with mock.patch.dict(os.environ, environ, clear=False):
            cells = self.battery.battery_cells("cell")

        self.assertEqual(len(cells), 1)
        cell = cells[0]
        self.assertEqual(cell.level, "cell")
        self.assertEqual(cell.profile, "upload")
        self.assertEqual(cell.images, 42)
        self.assertEqual(cell.ncpu, 2)
        self.assertEqual(cell.repo, str(self.repo))
        self.assertEqual(cell.env["WORKLOAD"], "upload")
        self.assertEqual(cell.env["IMAGES"], "42")
        self.assertEqual(cell.env["NCPU"], "2")
        self.assertEqual(cell.env["BUILD_PROFILE"], "debug")
        self.assertEqual(cell.env["SCAN_REPEATS"], "5")
        self.assertEqual(cell.env["UPLOAD_FILES"], "42")
        self.assertEqual(cell.outdir_suffix, "cell-upload")

    def test_list_includes_cell(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = self.battery.main(["list", "cell"])
        self.assertEqual(rc, 0)
        self.assertEqual(buf.getvalue().strip(), "cell-control")

    def test_defaults_are_applied(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            cell = self.battery.battery_cells("cell")[0]

        self.assertEqual(cell.profile, "control")
        self.assertEqual(cell.images, 100)
        self.assertEqual(cell.ncpu, 8)
        self.assertEqual(cell.env["BUILD_PROFILE"], "release")
        self.assertEqual(cell.env["IMAGES"], "100")
        self.assertEqual(cell.env["NCPU"], "8")
        self.assertEqual(cell.env["SCAN_REPEATS"], "2")
        self.assertEqual(cell.env["REPEATS"], str(self.battery.config.DEFAULT_REPEATS))
        self.assertEqual(cell.env["WARMUP"], "1")

    def test_bad_env_fails_before_run(self):
        for bad in (
            {"IMAGES": "0", "IMAGE_REPO": str(self.repo)},
            {"PORT": "abc", "IMAGE_REPO": str(self.repo)},
        ):
            with self.subTest(bad=bad):
                with mock.patch.dict(os.environ, bad, clear=False):
                    os.environ.pop("IMAGES", None) if "IMAGES" not in bad else None
                    with self.assertRaises(self.battery.config.ConfigError):
                        self.battery.battery_cells("cell")


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
            mock.patch.object(self.battery, "run_cell", FakeOrchestrator()),
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
            "scan_prefetch_p50_ms": self.battery.NA,
            "idle_thumb_p50_ms": self.battery.NA,
        }
        row_b = {
            "cell": "gate-control",
            "profile": "control",
            "level": "gate",
            "scale": "100",
            "build": "release",
            "commit": "abc",
            "scan_wall_s": "20.0",
            "scan_prefetch_p50_ms": "18.0",
            "idle_thumb_p50_ms": self.battery.NA,
        }
        row_c = {
            "cell": "gate-control",
            "profile": "control",
            "level": "gate",
            "scale": "100",
            "build": "release",
            "commit": "abc",
            "scan_wall_s": "30.0",
            "scan_prefetch_p50_ms": "22.0",
            "idle_thumb_p50_ms": self.battery.NA,
        }
        odd = self.battery._build_median_row([row_a, row_b, row_c])
        self.assertEqual(odd["cell"], "gate-control:median")
        self.assertEqual(odd["scan_wall_s"], "20.0")
        self.assertEqual(odd["scan_prefetch_p50_ms"], "20.0")
        self.assertEqual(odd["idle_thumb_p50_ms"], self.battery.NA)

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
            "scan_prefetch_p50_ms": "18.0",
        }
        row_b = {
            "cell": "gate-control",
            "filter_snapshot_clone_p50_us": self.battery.NA,
            "scan_prefetch_p50_ms": "20.0",
        }
        row_c = {
            "cell": "gate-control",
            "filter_snapshot_clone_p50_us": "425,1500",
            "scan_prefetch_p50_ms": self.battery.NA,
        }
        med = self.battery._build_median_row([row_a, row_b, row_c])
        # Pools [376, 1483, 425, 1500] and [18.0, 20.0]; the n/a repeats
        # contribute nothing and the all-n/a column stays n/a.
        self.assertEqual(float(med["filter_snapshot_clone_p50_us"]), 954.0)
        self.assertEqual(float(med["scan_prefetch_p50_ms"]), 19.0)
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
        # The recorded table's two scans repeat across the three reps:
        # p50s [542, 1002] x3 pool to a 772 median, maxes [1778, 5236]
        # x3 to 3507.
        self.assertEqual(float(med["tree_refresh_build_p50_us"]), 772.0)
        self.assertEqual(float(med["filter_snapshot_clone_max_us"]), 3507.0)

    def test_idle_tails_pool_every_post_scan_run_across_repeats(self):
        rows = [
            {
                "cell": "gate-control",
                "idle_prefetch_p95_ms": "8.8",
                "idle_prefetch_max_ms": "9.0",
                "samples": {"idle_prefetch": [6.0, 7.0, 9.0]},
            },
            {
                "cell": "gate-control",
                "idle_prefetch_p95_ms": "9.8",
                "idle_prefetch_max_ms": "11.0",
                "samples": {"idle_prefetch": [6.0, 7.0, 11.0]},
            },
        ]
        med = self.battery._build_median_row(rows)
        # Pooled samples [6, 6, 7, 7, 9, 11]: p95 = 9 + (11-9) * 0.75.
        self.assertEqual(med["idle_prefetch_p95_ms"], "10.5")
        self.assertEqual(med["idle_prefetch_max_ms"], "11.0")

    def test_headline_range_columns_report_min_and_max(self):
        rows = [
            {"cell": "gate-control", "scan_wall_s": "10.0", "temp_db_mb": "2.0"},
            {"cell": "gate-control", "scan_wall_s": "20.0", "temp_db_mb": "4.0"},
            {"cell": "gate-control", "scan_wall_s": "30.0", "temp_db_mb": "3.0"},
        ]
        med = self.battery._build_median_row(rows)
        self.assertEqual(med["scan_wall_s_min"], "10.0")
        self.assertEqual(med["scan_wall_s_max"], "30.0")
        self.assertEqual(med["temp_db_mb_min"], "2.0")
        self.assertEqual(med["temp_db_mb_max"], "4.0")

    def test_probe_tails_follow_the_per_repeat_values(self):
        # Each repeat already summarises its probe rounds; the recorded
        # worst-case baselines were medians of those per-rep values.
        rows = [
            {"cell": "gate-control", "scan_prefetch_max_ms": "14.3"},
            {"cell": "gate-control", "scan_prefetch_max_ms": "16.6"},
            {"cell": "gate-control", "scan_prefetch_max_ms": "13.0"},
        ]
        med = self.battery._build_median_row(rows)
        self.assertEqual(med["scan_prefetch_max_ms"], "14.3")

    def test_storm_columns_pool_every_run_across_repeats(self):
        def run(profile, kind, median, p95, maximum, failed=0):
            return {
                "profile": profile,
                "kind": kind,
                "median": median,
                "p95": p95,
                "max": maximum,
                "failed": failed,
            }

        rows = [
            {
                "cell": "gate-storm-read",
                "scan_storm_read_p50_ms": "2.5",
                "scan_storm_read_p95_ms": "15.0",
                "scan_storm_read_max_ms": "35.0",
                "scan_storm_read_failed": "0",
                "storms": [
                    run("storm-read", "read", 0.002, 0.013, 0.035),
                    run("storm-read", "read", 0.003, 0.015, 0.034),
                ],
            },
            {
                "cell": "gate-storm-read",
                "scan_storm_read_p50_ms": "4.0",
                "scan_storm_read_p95_ms": "15.0",
                "scan_storm_read_max_ms": "38.0",
                "scan_storm_read_failed": "0",
                "storms": [
                    run("storm-read", "read", 0.005, 0.015, 0.019),
                    run("storm-read", "read", 0.003, 0.010, 0.038),
                ],
            },
        ]
        med = self.battery._build_median_row(rows)
        # Run medians [2, 3, 5, 3] pool to 3; tails stay worst-case.
        self.assertEqual(med["scan_storm_read_p50_ms"], "3.0")
        self.assertEqual(med["scan_storm_read_p95_ms"], "15.0")
        self.assertEqual(med["scan_storm_read_max_ms"], "38.0")
        self.assertEqual(med["scan_storm_read_failed"], "0")

    def test_failed_counts_are_summed_across_repeats(self):
        rows = [
            {"cell": "gate-storm-diff", "scan_storm_edit_failed": "68"},
            {"cell": "gate-storm-diff", "scan_storm_edit_failed": "0"},
            {"cell": "gate-storm-diff", "scan_storm_edit_failed": "4"},
        ]
        med = self.battery._build_median_row(rows)
        self.assertEqual(med["scan_storm_edit_failed"], "72")


class BatteryCheckMedianTest(unittest.TestCase):
    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.report = self.root / "report.tsv"
        self.baselines_tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.baselines_tmp.cleanup)
        self.baselines = Path(self.baselines_tmp.name) / "baselines.tsv"

    def test_failing_repeat_with_passing_median_passes(self):
        rows = [
            {
                "cell": "gate-control",
                "profile": "control",
                "level": "gate",
                "scale": "100",
                "build": "release",
                "commit": "abc",
                "scan_prefetch_p50_ms": "40.0",
            },
            {
                "cell": "gate-control:median",
                "profile": "control",
                "level": "gate",
                "scale": "100",
                "build": "release",
                "commit": "abc",
                "scan_prefetch_p50_ms": "18.0",
            },
        ]
        self.battery.write_report(self.report, rows)
        self.baselines.write_text(
            self.battery.BASELINE_HEADER
            + "\n"
            + "gate-control:median\tscan_prefetch_p50_ms\t18.0\t2026-11-14\tgate\n"
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
                "scan_prefetch_p50_ms": "200.0",
            },
        ]
        self.battery.write_report(self.report, rows)
        self.baselines.write_text(
            self.battery.BASELINE_HEADER
            + "\n"
            + "gate-control:median\tscan_prefetch_p50_ms\t18.0\t2026-11-14\tgate\n"
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
            mock.patch.object(self.battery, "run_cell", FakeOrchestrator()),
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
        self.assertEqual(median["tree_refresh_build_p50_us"], "772.0")

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


class BatteryExtractTest(unittest.TestCase):
    """`battery extract` dumps one battery parent as a single JSON document.

    A battery keeps one parent directory holding `<level>-<profile>-repN`
    subdirs plus `report.tsv`; the extract walks the rep subdirs in sorted
    order and emits per-rep rows, manifest identity, stages, terminal
    state, pooled medians and the report TSV rows.
    """

    STAGES = [
        {
            "scan": 1,
            "scan_index": 1,
            "run": 1,
            "run_index": 0,
            "stage": "tree_refresh_build",
            "n": 100,
            "p50_us": 1200,
            "p95_us": 1500,
            "max_us": 2000,
            "context": "scan job=1",
        }
    ]

    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.parent = self.root / "gate-run"

    def extract(self, parent=None, out=None):
        argv = ["extract", str(self.parent if parent is None else parent)]
        if out is not None:
            argv += ["--out", str(out)]
        stdout = io.StringIO()
        with (
            contextlib.redirect_stdout(stdout),
            contextlib.redirect_stderr(io.StringIO()),
        ):
            rc = self.battery.main(argv)
        return rc, stdout.getvalue()

    def make_battery(self):
        """Two parseable reps of one cell, one broken dir, a report.tsv."""
        rep1 = self.parent / "gate-control-rep1"
        write_cell(rep1, walls=(11000, 13000))
        (rep1 / "run-manifest.json").write_text(
            json.dumps(
                {
                    "mode": "bench",
                    "port": 5795,
                    "build_profile": "release",
                    "git": "abc1234",
                    "dataset_id": "set-100",
                    "dataset_count": 100,
                    "observer_stats": "1",
                    "binary_sha": "deadbeef",
                    "machine": {"cpu_model": "test-cpu"},
                }
            )
        )
        rep2 = self.parent / "gate-control-rep2"
        write_cell(rep2, walls=(9000, 15000), drain=False, upload=False)
        (rep2 / "run-manifest.json").unlink()
        (rep2 / "status-final.json").write_text(
            json.dumps(
                {
                    "state": "failed",
                    "scanned": 100,
                    "matched": 100,
                    "processed": 100,
                    "failed": 0,
                }
            )
        )
        (self.parent / "gate-storm-read-rep1").mkdir(parents=True)
        rows = [
            self.battery.build_row("gate", "control", rep1),
            self.battery.build_row("gate", "control", rep2),
        ]
        rows.append(self.battery._build_median_row(rows))
        self.battery.write_report(self.parent / "report.tsv", rows)
        return rep1, rep2

    def test_environment_missing_gives_null(self):
        self.make_battery()

        rc, stdout = self.extract()

        self.assertEqual(rc, 0)
        self.assertIsNone(json.loads(stdout)["environment"])

    def test_environment_file_is_embedded_in_document(self):
        self.make_battery()
        (self.parent / "environment.json").write_text(json.dumps({"host": "benchbox"}))

        rc, stdout = self.extract()

        self.assertEqual(rc, 0)
        self.assertEqual(json.loads(stdout)["environment"], {"host": "benchbox"})

    def test_document_shape_lists_cells_sorted_and_counts_errors(self):
        self.make_battery()

        rc, stdout = self.extract()

        self.assertEqual(rc, 0)  # partial battery: two reps parsed, one broken
        doc = json.loads(stdout)
        self.assertEqual(
            list(doc),
            ["battery", "level", "cells", "medians", "report", "errors", "environment"],
        )
        self.assertEqual(doc["battery"], str(self.parent))
        self.assertEqual(doc["level"], "gate")
        self.assertEqual(doc["errors"], 1)
        self.assertEqual(
            [entry["dir"] for entry in doc["cells"]],
            ["gate-control-rep1", "gate-control-rep2", "gate-storm-read-rep1"],
        )
        ok = doc["cells"][0]
        self.assertEqual(list(ok), ["dir", "row", "manifest", "stages", "terminal"])
        self.assertEqual(ok["row"]["cell"], "gate-control")
        # The entry's row is the report row, not the build_row dict.
        self.assertEqual(list(ok["row"]), self.battery.REPORT_COLUMNS)
        broken = doc["cells"][2]
        self.assertEqual(list(broken), ["dir", "error"])
        self.assertTrue(broken["error"])
        json.dumps(doc, allow_nan=False)  # strict JSON, no NaN/Infinity

    def test_manifest_identity_is_a_subset_and_absent_manifest_is_null(self):
        self.make_battery()

        _rc, stdout = self.extract()

        doc = json.loads(stdout)
        self.assertEqual(
            doc["cells"][0]["manifest"],
            {
                "dataset_id": "set-100",
                "dataset_count": 100,
                "commit": "abc1234",
                "build_profile": "release",
                "observer": "1",
                "mode": "bench",
                "binary_sha": "deadbeef",
            },
        )
        self.assertEqual(
            doc["cells"][1]["manifest"],
            {key: None for key in doc["cells"][0]["manifest"]},
        )

    def test_corrupt_manifest_is_never_fatal(self):
        outdir = self.root / "gate-control-rep1"
        outdir.mkdir()
        (outdir / "run-manifest.json").write_text("{")

        identity = self.battery._manifest_identity(outdir)

        self.assertEqual(
            identity,
            {
                "dataset_id": None,
                "dataset_count": None,
                "commit": None,
                "build_profile": None,
                "observer": None,
                "mode": None,
                "binary_sha": None,
            },
        )

    def test_stages_and_terminal_state_come_from_rep_artifacts(self):
        self.make_battery()

        _rc, stdout = self.extract()

        doc = json.loads(stdout)
        # rep1's drain tables live in server.log only: the extract carries
        # them without a `stages.json` a standalone run would have written.
        stages = doc["cells"][0]["stages"]
        self.assertTrue(stages)
        self.assertEqual(len(stages), 24)
        self.assertEqual(stages[0]["stage"], "filter_snapshot_clone")
        self.assertEqual(sorted({stage["scan"] for stage in stages}), [1, 2])
        # rep2 carries neither drain tables nor a stages.json.
        self.assertEqual(doc["cells"][1]["stages"], [])
        self.assertEqual(doc["cells"][0]["terminal"], "completed")
        self.assertEqual(doc["cells"][1]["terminal"], "failed")
        empty = self.root / "empty-rep1"
        empty.mkdir()
        self.assertEqual(self.battery._terminal_state(empty), "incomplete")

    def test_stages_fall_back_to_stages_json_without_drain_tables(self):
        rep = self.parent / "gate-control-rep1"
        write_cell(rep, drain=False, upload=False)
        (rep / "stages.json").write_text(json.dumps(self.STAGES))

        rc, stdout = self.extract()

        self.assertEqual(rc, 0)
        self.assertEqual(json.loads(stdout)["cells"][0]["stages"], self.STAGES)

    def test_medians_pool_rep_rows_of_the_same_cell(self):
        self.make_battery()

        _rc, stdout = self.extract()

        doc = json.loads(stdout)
        self.assertEqual(len(doc["medians"]), 1)
        median = doc["medians"][0]
        self.assertEqual(median["cell"], "gate-control:median")
        # Pooled [9, 11, 13, 15] s across both reps.
        self.assertEqual(median["scan_walls_s"], "12.0")

    def test_report_section_carries_report_tsv_rows(self):
        self.make_battery()

        _rc, stdout = self.extract()

        doc = json.loads(stdout)
        report = doc["report"]
        self.assertEqual(
            [row["cell"] for row in report],
            ["gate-control", "gate-control", "gate-control:median"],
        )
        self.assertEqual(report[0]["scan_walls_s"], "11.0,13.0")
        self.assertEqual(report[1]["scan_walls_s"], "9.0,15.0")
        self.assertEqual(report[-1]["scan_walls_s"], "12.0")
        self.assertEqual(list(report[0]), self.battery.REPORT_COLUMNS)

    def test_missing_report_tsv_is_an_empty_list(self):
        self.make_battery()
        (self.parent / "report.tsv").unlink()

        rc, stdout = self.extract()

        self.assertEqual(rc, 0)
        self.assertEqual(json.loads(stdout)["report"], [])

    def test_report_from_an_older_schema_is_omitted(self):
        self.make_battery()
        (self.parent / "report.tsv").write_text("cell\told_column\ngate-control\t1\n")
        stderr = io.StringIO()

        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
            rc = self.battery.main(["extract", str(self.parent)])

        self.assertEqual(rc, 0)
        self.assertIn("unexpected report header", stderr.getvalue())

    def test_out_writes_the_file_and_leaves_stdout_empty(self):
        self.make_battery()
        out = self.root / "nested" / "run.json"

        rc, stdout = self.extract(out=out)

        self.assertEqual(rc, 0)
        self.assertEqual(stdout, "")
        doc = json.loads(out.read_text())
        self.assertEqual(doc["battery"], str(self.parent))
        self.assertEqual(doc["errors"], 1)

    def test_exit_codes_for_empty_and_fully_broken_parents(self):
        empty = self.root / "no-reps"
        empty.mkdir()
        (empty / "report.tsv").write_text("stray file, not a rep dir\n")
        (empty / "notes").mkdir()

        rc, stdout = self.extract(empty)
        self.assertEqual(rc, 1)
        self.assertEqual(stdout, "")

        rc, stdout = self.extract(self.root / "missing")
        self.assertEqual(rc, 1)
        self.assertEqual(stdout, "")

        broken = self.root / "all-broken"
        (broken / "gate-control-rep1").mkdir(parents=True)

        rc, stdout = self.extract(broken)
        self.assertEqual(rc, 2)
        doc = json.loads(stdout)
        self.assertEqual(doc["errors"], 1)
        self.assertIsNone(doc["level"])
        self.assertEqual(doc["medians"], [])
        self.assertEqual(doc["report"], [])

    def test_rep_dir_without_cell_name_is_an_error_entry(self):
        (self.parent / "flat-rep1").mkdir(parents=True)

        rc, stdout = self.extract()

        self.assertEqual(rc, 2)
        doc = json.loads(stdout)
        self.assertEqual(doc["errors"], 1)
        self.assertEqual(doc["cells"][0]["dir"], "flat-rep1")
        self.assertTrue(doc["cells"][0]["error"])

    def test_nonfinite_stage_values_become_null_and_na_stays_string(self):
        rep = self.parent / "gate-control-rep1"
        write_cell(rep, drain=False, upload=False)
        (rep / "stages.json").write_text(
            json.dumps([{"stage": "tree_refresh_build", "n": 1, "p50_us": float("inf")}])
        )

        rc, stdout = self.extract()

        self.assertEqual(rc, 0)
        self.assertNotIn("Infinity", stdout)
        doc = json.loads(stdout)
        self.assertIsNone(doc["cells"][0]["stages"][0]["p50_us"])
        self.assertEqual(doc["cells"][0]["row"]["upload_submit_p50_ms"], self.battery.NA)
        json.dumps(doc, allow_nan=False)


class MatrixRollupTest(unittest.TestCase):
    """Battery-level rollup math over the extract's median rows.

    The renderer derives every view cell from the `<cell>:median` rows:
    contributing profile medians roll up to their median with the
    contributor count recorded, a single contributor passes through, and
    a cell nothing measured stays `n/a`.
    """

    def setUp(self):
        self.battery = load_battery()

    @staticmethod
    def row(**cells):
        return {"cell": "gate-control:median", **cells}

    def test_odd_contributor_count_is_the_median_across_profiles(self):
        rows = [
            self.row(idle_prefetch_p50_ms="20.0"),
            self.row(idle_prefetch_p50_ms="10.0"),
            self.row(idle_prefetch_p50_ms="40.0"),
        ]

        rollup = self.battery.matrix_rollup(rows)

        self.assertEqual(rollup[("p50", "idle", "prefetch")], ("20.0", 3))

    def test_even_contributor_count_averages_the_middle_pair(self):
        rows = [
            self.row(idle_edit_p50_ms="39.0"),
            self.row(idle_edit_p50_ms="42.0"),
            self.row(idle_edit_p50_ms="41.0"),
            self.row(idle_edit_p50_ms="40.0"),
        ]

        rollup = self.battery.matrix_rollup(rows)

        self.assertEqual(rollup[("p50", "idle", "edit")], ("40.5", 4))

    def test_single_contributor_passes_the_recorded_string_through(self):
        rows = [self.row(scan_prefetch_p95_ms="18.0")]

        rollup = self.battery.matrix_rollup(rows)

        self.assertEqual(rollup[("p95", "scan", "prefetch")], ("18.0", 1))

    def test_cell_without_a_contributor_is_na_with_count_zero(self):
        rollup = self.battery.matrix_rollup([self.row(scan_prefetch_p50_ms="18.0")])

        self.assertEqual(rollup[("p50", "uscan", "storm_read")], (self.battery.NA, 0))
        self.assertEqual(rollup[("max", "scan", "storm_thumb")], (self.battery.NA, 0))

    def test_na_empty_and_non_numeric_values_do_not_contribute(self):
        rows = [
            self.row(
                idle_thumb_p50_ms="n/a",
                idle_edit_p50_ms="",
                scan_thumb_p50_ms="junk",
                scan_edit_p50_ms=float("nan"),
            ),
            {"cell": "gate-upload:median"},  # the key is absent entirely
        ]

        rollup = self.battery.matrix_rollup(rows)

        for key in (
            ("p50", "idle", "thumb"),
            ("p50", "idle", "edit"),
            ("p50", "scan", "thumb"),
            ("p50", "scan", "edit"),
        ):
            self.assertEqual(rollup[key], (self.battery.NA, 0), key)


class BatteryMatrixViewTest(unittest.TestCase):
    """The extract's human-readable artifacts: `report.md` (the report:
    header, matrices, anomalies, baseline conformance) and `matrix.tsv`
    (the view matrices as a spreadsheet-readable TSV), both derived from
    the median rows.

    The fixture battery spans four profiles whose median rows fill
    different parts of the background x access matrix — control (scan +
    idle probes), upload (uscan + idle), storm-read (scan storms + idle)
    and storm-idle (idle storms + idle) — so every rollup shape shows
    up: multi-contributor cells, single contributors and empty cells.
    """

    SPECS = (
        ("gate-control", {"post": (0.014, 0.007, 0.039)}),
        (
            "gate-upload",
            {"profile": "upload", "post": (0.024, 0.011, 0.041), "probe": (0.020, 0.007, 0.030)},
        ),
        (
            "gate-storm-read",
            {
                "profile": "storm-read",
                "probe": None,
                "post": (0.009, 0.006, 0.040),
                "storm": ("storm-read", 0.002, 0.013, 0.035),
            },
        ),
        (
            "gate-storm-idle",
            {
                "profile": "storm-idle",
                "probe": None,
                "post": (0.012, 0.009, 0.042),
                "storm": ("storm-idle", 0.004, 0.020, 0.050),
            },
        ),
    )

    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.parent = self.root / "gate-run"
        for cell, kwargs in self.SPECS:
            for rep in (1, 2):
                write_cell(self.parent / f"{cell}-rep{rep}", **kwargs)

    def extract(self, *extra, out=None):
        argv = ["extract", str(self.parent)]
        if out is not None:
            argv += ["--out", str(out)]
        argv += list(extra)
        stdout = io.StringIO()
        with (
            contextlib.redirect_stdout(stdout),
            contextlib.redirect_stderr(io.StringIO()),
        ):
            rc = self.battery.main(argv)
        return rc, stdout.getvalue()

    @staticmethod
    def sections(text):
        """Split the matrix TSV into `{title: [field rows]}`."""
        sections: dict = {}
        title = None
        for line in text.splitlines():
            if not line.strip():
                continue
            if line.startswith("# "):
                title = line[2:]
                sections[title] = []
            else:
                sections[title].append(line.split("\t"))
        return sections

    @staticmethod
    def md_table(text, title):
        """Rows of the `<title>` Markdown table as field lists (any level)."""
        lines = text.splitlines()
        start = next(
            index
            for index, line in enumerate(lines)
            if line.startswith("#") and line.lstrip("#").strip() == title
        )
        rows = []
        for line in lines[start + 1 :]:
            if not line.startswith("|"):
                if rows:
                    break
                continue
            rows.append([cell.strip() for cell in line.strip("|").split("|")])
        # Drop the `| --- |` separator row.
        return [row for row in rows if not all(set(cell) <= {"-", ":"} for cell in row)]

    def value_row(self, text, stat, access):
        """One access row of a value section as `{background: value}`."""
        lines = self.sections(text)[f"{stat} (ms)"]
        self.assertEqual(lines[0], ["access", "idle", "scan", "uscan"])
        row = next(fields for fields in lines[1:] if fields[0] == access)
        return dict(zip(["idle", "scan", "uscan"], row[1:], strict=True))

    def count_row(self, text, stat, access):
        """One access row of the contributor section as `{background: n}`."""
        lines = self.sections(text)["contributors"]
        self.assertEqual(lines[0], ["stat", "access", "idle", "scan", "uscan"])
        row = next(fields for fields in lines[1:] if fields[0] == stat and fields[1] == access)
        return dict(zip(["idle", "scan", "uscan"], row[2:], strict=True))

    def test_matrix_artifacts_land_next_to_the_json(self):
        out = self.root / "nested" / "run.json"

        rc, stdout = self.extract(out=out)

        self.assertEqual(rc, 0)
        self.assertEqual(stdout, "")  # JSON went to the file, not stdout
        self.assertTrue((out.parent / "report.md").is_file())
        self.assertTrue((out.parent / "matrix.tsv").is_file())
        sections = self.sections((out.parent / "matrix.tsv").read_text())
        self.assertEqual(list(sections), ["p50 (ms)", "p95 (ms)", "max (ms)", "contributors"])

    def test_matrix_artifacts_land_in_the_battery_parent_without_out(self):
        rc, stdout = self.extract()

        self.assertEqual(rc, 0)
        json.loads(stdout)  # stdout still carries the JSON document
        self.assertTrue((self.parent / "report.md").is_file())
        self.assertTrue((self.parent / "matrix.tsv").is_file())
        # The extract's own artifacts never make the parent foreign.
        self.assertEqual(self.battery.foreign_entries(self.parent), [])

    def test_p50_matrix_rolls_up_the_contributing_profile_medians(self):
        rc, _stdout = self.extract()
        self.assertEqual(rc, 0)
        text = (self.parent / "matrix.tsv").read_text()

        # idle: four contributors (control, upload, storm-read, storm-idle)
        # roll up to the median of their medians; the middle pair averages.
        self.assertEqual(self.value_row(text, "p50", "prefetch")["idle"], "13.0")  # 12,14
        self.assertEqual(self.value_row(text, "p50", "thumb")["idle"], "8.0")  # 7,9
        self.assertEqual(self.value_row(text, "p50", "edit")["idle"], "40.5")  # 40,41
        # scan: only control ran the during-scan probes -> pass-through.
        self.assertEqual(self.value_row(text, "p50", "prefetch")["scan"], "18.0")
        self.assertEqual(self.value_row(text, "p50", "thumb")["scan"], "6.0")
        self.assertEqual(self.value_row(text, "p50", "edit")["scan"], "28.0")
        # uscan: only the upload profile fills that background.
        self.assertEqual(self.value_row(text, "p50", "prefetch")["uscan"], "20.0")
        self.assertEqual(self.value_row(text, "p50", "thumb")["uscan"], "7.0")
        self.assertEqual(self.value_row(text, "p50", "edit")["uscan"], "30.0")

    def test_storm_rows_carry_their_profiles_and_unmeasured_cells_are_na(self):
        rc, _stdout = self.extract()
        self.assertEqual(rc, 0)
        text = (self.parent / "matrix.tsv").read_text()

        self.assertEqual(self.value_row(text, "p50", "storm_read")["scan"], "2.0")
        self.assertEqual(self.value_row(text, "p50", "storm_read")["idle"], "4.0")
        self.assertEqual(self.value_row(text, "p95", "storm_read")["scan"], "13.0")
        self.assertEqual(self.value_row(text, "p95", "storm_read")["idle"], "20.0")
        self.assertEqual(self.value_row(text, "max", "storm_read")["scan"], "35.0")
        self.assertEqual(self.value_row(text, "max", "storm_read")["idle"], "50.0")
        # Upload never runs a storm; no storm-thumb/diff profile ran.
        for stat in ("p50", "p95", "max"):
            for access in ("storm_thumb", "storm_edit"):
                self.assertEqual(
                    self.value_row(text, stat, access),
                    {"idle": "n/a", "scan": "n/a", "uscan": "n/a"},
                    f"{stat}/{access}",
                )
            self.assertEqual(self.value_row(text, stat, "storm_read")["uscan"], "n/a", stat)

    def test_contributor_counts_record_how_many_profiles_filled_each_cell(self):
        rc, _stdout = self.extract()
        self.assertEqual(rc, 0)
        text = (self.parent / "matrix.tsv").read_text()

        for stat in ("p50", "p95", "max"):
            self.assertEqual(
                self.count_row(text, stat, "prefetch"),
                {"idle": "4", "scan": "1", "uscan": "1"},
                stat,
            )
            self.assertEqual(
                self.count_row(text, stat, "storm_read"),
                {"idle": "1", "scan": "1", "uscan": "0"},
                stat,
            )
            self.assertEqual(
                self.count_row(text, stat, "storm_edit"),
                {"idle": "0", "scan": "0", "uscan": "0"},
                stat,
            )

    def test_matrix_output_is_byte_identical_across_runs(self):
        first = self.root / "first"
        second = self.root / "second"

        rc1, _ = self.extract(out=first / "run.json")
        rc2, _ = self.extract(out=second / "run.json")
        rc3, _ = self.extract()  # default: JSON on stdout, artifacts in the parent

        self.assertEqual((rc1, rc2, rc3), (0, 0, 0))
        for name in ("report.md", "matrix.tsv"):
            expected = (first / name).read_bytes()
            self.assertEqual(expected, (second / name).read_bytes(), name)
            self.assertEqual(expected, (self.parent / name).read_bytes(), name)

    def test_matrix_only_prints_the_report_instead_of_the_json(self):
        rc, stdout = self.extract("--matrix-only")

        self.assertEqual(rc, 0)
        self.assertTrue(stdout.startswith("# Battery report\n"))
        self.assertNotIn('"cells"', stdout)
        with self.assertRaises(json.JSONDecodeError):
            json.loads(stdout)
        # The file artifact is still written for readers.
        self.assertEqual(stdout, (self.parent / "report.md").read_text())

    def test_matrix_only_prints_the_tsv_with_format_tsv(self):
        rc, stdout = self.extract("--matrix-only", "--format", "tsv")

        self.assertEqual(rc, 0)
        self.assertTrue(stdout.startswith("# p50 (ms)\n"))
        self.assertEqual(stdout, (self.parent / "matrix.tsv").read_text())

    def test_matrix_only_keeps_the_json_file_when_out_is_given(self):
        out = self.root / "run.json"

        rc, stdout = self.extract("--matrix-only", out=out)

        self.assertEqual(rc, 0)
        self.assertTrue(stdout.startswith("# Battery report\n"))
        self.assertEqual(json.loads(out.read_text())["level"], "gate")

    def test_format_md_writes_only_the_report(self):
        out = self.root / "md-only" / "run.json"

        rc, stdout = self.extract("--format", "md", out=out)

        self.assertEqual(rc, 0)
        self.assertEqual(stdout, "")
        self.assertTrue((out.parent / "report.md").is_file())
        self.assertFalse((out.parent / "matrix.tsv").exists())

    def test_format_tsv_writes_only_the_matrix(self):
        out = self.root / "tsv-only" / "run.json"

        rc, stdout = self.extract("--format", "tsv", out=out)

        self.assertEqual(rc, 0)
        self.assertEqual(stdout, "")
        self.assertTrue((out.parent / "matrix.tsv").is_file())
        self.assertFalse((out.parent / "report.md").exists())

    def test_report_tables_carry_the_same_values_as_the_tsv(self):
        rc, _stdout = self.extract()
        self.assertEqual(rc, 0)
        tsv = (self.parent / "matrix.tsv").read_text()
        md = (self.parent / "report.md").read_text()

        for section in ("p50 (ms)", "p95 (ms)", "max (ms)", "contributors"):
            self.assertEqual(self.md_table(md, section), self.sections(tsv)[section], section)

    def test_report_header_carries_identity_and_outcomes(self):
        rc, _stdout = self.extract()
        self.assertEqual(rc, 0)
        md = (self.parent / "report.md").read_text()

        self.assertTrue(md.startswith("# Battery report\n\n"))
        today = datetime.date.today().isoformat()
        for line in (
            f"- battery: `{self.parent}`",
            "- level: `gate`",
            "- scale: `100`",
            "- build: `release`",
            "- commit: `abc1234`",
            f"- date: `{today}`",
            "- outcomes: completed=8, failed=0, incomplete=0, errors=0",
            "- failures: none",
        ):
            self.assertIn(line + "\n", md, line)

    def test_report_anomalies_rank_the_worst_value_of_each_stat(self):
        rc, _stdout = self.extract()
        self.assertEqual(rc, 0)
        md = (self.parent / "report.md").read_text()

        rows = self.md_table(md, "worst values")
        self.assertEqual(rows[0], ["stat", "value (ms)", "cell", "condition"])
        worst = {fields[0]: fields[1:] for fields in rows[1:]}
        self.assertEqual(worst["p50"], ["42.0", "gate-storm-idle:median", "idle/edit"])
        self.assertEqual(worst["p95"], ["42.0", "gate-storm-idle:median", "idle/edit"])
        self.assertEqual(worst["max"], ["50.0", "gate-storm-idle:median", "idle/storm_read"])

    def test_report_does_not_flag_structurally_single_contributor_cells(self):
        rc, _stdout = self.extract()
        self.assertEqual(rc, 0)
        md = (self.parent / "report.md").read_text()

        # Shared idle probes are filled by every profile (4 contributors), so
        # no shared cell is thin; single-contributor columns (scan_*, uscan_*,
        # storm kinds) are never flagged by design.
        self.assertIn("### contributor counts below 2\n\n- none\n", md)

    def test_report_reports_no_missing_data_when_every_expectation_is_filled(self):
        rc, _stdout = self.extract()
        self.assertEqual(rc, 0)
        md = (self.parent / "report.md").read_text()

        self.assertIn("### expected data missing\n\n- none\n", md)


class BatteryReportViewTest(unittest.TestCase):
    """`report.md` sections driven by outcomes, missing data and baselines.

    control (probes and drain present) next to storm-thumb, whose reps
    recorded no storm summary and no drain tables — the anomalies
    section must name exactly the columns that profile was expected to
    fill — plus one unparsable rep dir for the outcome/failure lines.
    The baseline conformance renders the existing check path's result.
    """

    md_table = staticmethod(BatteryMatrixViewTest.md_table)

    SPECS = (
        ("gate-control", {}),
        (
            "gate-storm-thumb",
            {
                "profile": "storm-thumb",
                "probe": None,
                "drain": False,
                "post": (0.009, 0.006, 0.040),
            },
        ),
    )

    MISSING = [
        "gate-storm-thumb:median: scan_storm_thumb_p50_ms = n/a",
        "gate-storm-thumb:median: scan_storm_thumb_p95_ms = n/a",
        "gate-storm-thumb:median: scan_storm_thumb_max_ms = n/a",
        "gate-storm-thumb:median: scan_storm_thumb_failed = n/a",
        "gate-storm-thumb:median: tree_refresh_build_p50_us = n/a",
        "gate-storm-thumb:median: tree_refresh_build_max_us = n/a",
        "gate-storm-thumb:median: filter_snapshot_clone_p50_us = n/a",
        "gate-storm-thumb:median: filter_snapshot_clone_max_us = n/a",
        "gate-storm-thumb:median: filter_par_iter_p50_us = n/a",
        "gate-storm-thumb:median: filter_par_iter_max_us = n/a",
        "gate-storm-thumb:median: compute_locate_p50_us = n/a",
        "gate-storm-thumb:median: compute_locate_max_us = n/a",
        "gate-storm-thumb:median: tree_refresh_total_p50_us = n/a",
        "gate-storm-thumb:median: tree_refresh_total_max_us = n/a",
        "gate-storm-thumb:median: tree_refresh_sort_p50_us = n/a",
        "gate-storm-thumb:median: tree_refresh_sort_max_us = n/a",
        "gate-storm-thumb:median: tree_refresh_write_wait_p50_us = n/a",
        "gate-storm-thumb:median: tree_refresh_write_wait_max_us = n/a",
        "gate-storm-thumb:median: prefetch_total_p50_us = n/a",
        "gate-storm-thumb:median: prefetch_total_max_us = n/a",
    ]

    def setUp(self):
        self.battery = load_battery()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.parent = self.root / "gate-run"
        for cell, kwargs in self.SPECS:
            for rep in (1, 2):
                write_cell(self.parent / f"{cell}-rep{rep}", **kwargs)
        # A rep dir whose artifacts no longer parse: an error entry, not a crash.
        (self.parent / "gate-upload-rep1").mkdir(parents=True)

    def report(self):
        stdout = io.StringIO()
        with (
            contextlib.redirect_stdout(stdout),
            contextlib.redirect_stderr(io.StringIO()),
        ):
            rc = self.battery.main(["extract", str(self.parent)])
        self.assertEqual(rc, 0)
        return (self.parent / "report.md").read_text()

    @staticmethod
    def bullet_lines(md, heading):
        """`- ` bullet bodies under one heading, until the next heading."""
        lines = md.splitlines()
        start = lines.index(heading)
        bullets = []
        for line in lines[start + 1 :]:
            if line.startswith("#"):
                break
            if line.startswith("- "):
                bullets.append(line[2:])
        return bullets

    def test_header_names_every_rep_that_did_not_complete(self):
        md = self.report()

        self.assertIn("- outcomes: completed=4, failed=0, incomplete=0, errors=1\n", md)
        line = next(line for line in md.splitlines() if line.startswith("- failures:"))
        self.assertIn("gate-upload-rep1: error (", line)
        # The four parsed reps completed; only the broken dir is a failure.
        self.assertNotIn("gate-control", line)
        self.assertNotIn("gate-storm-thumb", line)

    def test_missing_expected_data_names_the_cell_and_column(self):
        md = self.report()

        missing = self.bullet_lines(md, "### expected data missing")
        self.assertEqual(missing, self.MISSING)

    def test_contributor_section_ignores_single_contributor_and_missing_storms(self):
        md = self.report()

        # scan_*/uscan_*/storm kinds are single-contributor by design and never
        # flagged; the shared idle probes are filled by both profiles here.
        self.assertIn("### contributor counts below 2\n\n- none\n", md)

    def test_worst_values_rank_the_largest_single_median_row_value(self):
        md = self.report()

        rows = self.md_table(md, "worst values")
        worst = {fields[0]: fields[1:] for fields in rows[1:]}
        for stat in ("p50", "p95", "max"):
            self.assertEqual(worst[stat], ["40.0", "gate-storm-thumb:median", "idle/edit"], stat)


if __name__ == "__main__":
    unittest.main()
