#!/usr/bin/env python3
"""Battery definition, recording and reporting behaviour."""

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))

import battery  # noqa: E402
import config  # noqa: E402
import metrics  # noqa: E402
import scenarios  # noqa: E402

CELL = f"{scenarios.PROBE}-100"
SMALL_CELL = f"{scenarios.PROBE}-50"


def sample_record(
    cell=CELL,
    profile=scenarios.PROBE,
    images=100,
    samples=None,
    source="battery",
):
    return {
        "cell": cell,
        "identity": {
            "cell": cell,
            "source": source,
            "profile": profile,
            "images": images,
            "upload_files": None,
            "build": "release",
            "commit": "abc1234",
            "scan_runs": 2,
        },
        "validity": {
            "reps": 1,
            "probe_rounds_total": 4,
            "probe_rounds_valid": 4,
            "storm_coverage_pct": "n/a",
            "warmup_valid": "true",
            "phase_invalid": 0,
            "phase_invalid_reasons": "n/a",
        },
        "samples": samples
        or {
            metrics.metric_key("latency", "idle", "prefetch"): [6.0, 7.0],
            metrics.metric_key("wall", "scan", "scan"): [5.0, 6.0],
        },
        "clusters": {
            metrics.metric_key("latency", "idle", "prefetch"): 2,
            metrics.metric_key("wall", "scan", "scan"): 2,
        },
    }


class DebugArgvTest(unittest.TestCase):
    def test_run_cell_selects_the_debug_path(self):
        argv, debug = battery._run_argv(["run", "cell", "--out", "/tmp/x"])
        self.assertTrue(debug)
        self.assertNotIn("cell", argv)

    def test_run_images_is_a_battery(self):
        argv, debug = battery._run_argv(["run", "--images", "100", "--out", "/tmp/x"])
        self.assertFalse(debug)
        self.assertEqual(argv, ["run", "--images", "100", "--out", "/tmp/x"])


class CellDefinitionTest(unittest.TestCase):
    def test_full_matrix_covers_every_profile_at_the_requested_image_count(self):
        cells = battery.battery_cells(100)
        profiles = {cell.profile for cell in cells}
        self.assertEqual(profiles, set(scenarios.profiles()))
        self.assertEqual(len(cells), len(scenarios.profiles()))
        self.assertEqual(len({cell.cell_id for cell in cells}), len(cells))
        for cell in cells:
            self.assertEqual(cell.images, 100)
            self.assertEqual(cell.cell_id, f"{cell.profile}-{cell.images}")
        storm_profiles = {cell.profile for cell in cells if scenarios.is_storm(cell.profile)}
        self.assertEqual(storm_profiles, set(scenarios.storm_profiles()))
        self.assertEqual(
            {scenarios.storm_parts(profile)[0] for profile in storm_profiles}, {"scan", "idle"}
        )
        self.assertEqual(
            {scenarios.storm_parts(profile)[1] for profile in storm_profiles},
            set(scenarios.STORM_KINDS),
        )
        self.assertEqual(
            {scenarios.storm_parts(profile)[2] for profile in storm_profiles},
            set(scenarios.STORM_LEVELS),
        )

    def test_repo_follows_image_count(self):
        cells = battery.battery_cells(5650)
        self.assertEqual({cell.repo for cell in cells}, {"set-5650"})

    def test_both_upload_cells_declare_their_source(self):
        cells = [
            cell
            for cell in battery.battery_cells(100)
            if cell.profile in (scenarios.IDLE_UPLOAD, scenarios.SCAN_UPLOAD)
        ]
        self.assertEqual(len(cells), 2)
        for cell in cells:
            with self.subTest(profile=cell.profile):
                self.assertEqual(cell.env["UPLOAD_FILES"], str(config.DEFAULT_UPLOAD_FILES))
                self.assertEqual(cell.env["UPLOAD_REPO"], "set-upload")

    def test_storm_cell_clients_follow_the_level(self):
        for cell in battery.battery_cells(100):
            if not scenarios.is_storm(cell.profile):
                continue
            with self.subTest(profile=cell.profile):
                _background, _kind, level = scenarios.storm_parts(cell.profile)
                self.assertEqual(
                    cell.env["STORM_CLIENTS"], str(scenarios.level_clients(level, cell.ncpu))
                )

    def test_only_the_probe_cell_carries_a_round_budget(self):
        for profile in scenarios.profiles():
            spec = scenarios.build_spec(
                profile,
                min_probe_rounds=20,
                repeats=3,
                upload_files=20,
                storm_ops=10,
                ncpu=8,
            )
            with self.subTest(profile=profile):
                if profile == scenarios.PROBE:
                    self.assertEqual(spec.probe_round_target, 20)
                else:
                    self.assertEqual(spec.probe_round_target, 0)

    def test_list_prints_a_line_for_every_cell(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = battery.main(["list", "--images", "200"])
        self.assertEqual(rc, 0)
        printed = buf.getvalue()
        for profile in scenarios.profiles():
            self.assertIn(f"{profile}-200", printed)
        self.assertNotIn("gate", printed)


class ExtractEntryTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_profile_and_images_come_from_the_cell_name(self):
        path = self.root / f"{CELL}-rep2"
        path.mkdir()
        with mock.patch.object(battery, "build_row", return_value=sample_record()):
            entry = battery._extract_entry(path)
        self.assertNotIn("error", entry)
        self.assertEqual(entry["row"]["cell"], CELL)

    def test_unknown_directory_is_an_error_entry(self):
        path = self.root / "not-a-cell-rep1"
        path.mkdir()
        entry = battery._extract_entry(path)
        self.assertIn("error", entry)


class ReuseTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.cell = battery._cell(scenarios.PROBE, 100, "set-100")

    def test_missing_status_is_not_reused(self):
        self.assertIsNone(battery._reuse_row(self.cell, self.root))

    def test_completed_rep_is_reused(self):
        (self.root / "status-final.json").write_text('{"state": "completed"}')
        record = sample_record()
        with mock.patch.object(battery, "build_row", return_value=record) as build:
            row = battery._reuse_row(self.cell, self.root)
        self.assertIs(row, record)
        build.assert_called_once()
        self.assertEqual(build.call_args.kwargs["cell_id"], self.cell.cell_id)

    def test_unparseable_rep_is_not_reused(self):
        (self.root / "status-final.json").write_text('{"state": "completed"}')
        with mock.patch.object(battery, "build_row", side_effect=ValueError("bad")):
            self.assertIsNone(battery._reuse_row(self.cell, self.root))


class PoolingTest(unittest.TestCase):
    def test_samples_pool_across_reps_and_stats_share_the_pool(self):
        rows, identity, validity = battery._pooled_report_rows(
            [
                sample_record(),
                sample_record(
                    samples={
                        metrics.metric_key("latency", "idle", "prefetch"): [8.0],
                    }
                ),
            ]
        )
        by_stat = {
            row["stat"]: row
            for row in rows
            if row["family"] == "latency" and row["subject"] == "prefetch"
        }
        self.assertEqual(by_stat["p50"]["value"], "7.0")
        self.assertEqual(by_stat["p50"]["n_samples"], 3)
        self.assertEqual(by_stat["max"]["value"], "8.0")
        self.assertEqual(identity["cell"], CELL)
        self.assertEqual(validity["reps"], 2)

    def test_counts_stay_integers(self):
        key = metrics.metric_key("count", "scan", "prefetch")
        rows, _identity, _validity = battery._pooled_report_rows(
            [sample_record(samples={key: [0.0, 2.0]})]
        )
        self.assertEqual([row["value"] for row in rows], ["2"])
        self.assertEqual(rows[0]["unit"], "ops")


class BaselineMergeTest(unittest.TestCase):
    def _row(self, cell=CELL, subject="prefetch", stat="p50", value="7.0"):
        return {
            "cell": cell,
            "family": "latency",
            "background": "idle",
            "subject": subject,
            "stat": stat,
            "value": value,
            "unit": "ms",
            "n_samples": "2",
            "n_clusters": "2",
            "ci_low": "6.9",
            "ci_high": "7.1",
        }

    def test_notes_survive_and_other_cells_carry_over(self):
        existing = {
            **self._row(),
            "recorded": "old",
            "note": "why it moved",
        }
        carried = {
            **self._row(cell=SMALL_CELL),
            "recorded": "old",
            "note": "kept",
        }
        merged, new_metrics = battery.merge_baselines(
            [self._row(value="7.2")],
            [existing, carried],
            run_date="2026-01-01",
            branch="main",
            commit="abc1234",
        )
        by_cell = {row["cell"]: row for row in merged}
        self.assertEqual(by_cell[CELL]["note"], "why it moved")
        self.assertEqual(by_cell[CELL]["value"], "7.2")
        self.assertEqual(by_cell[CELL]["recorded"], "2026-01-01 main abc1234")
        self.assertEqual(by_cell[SMALL_CELL]["note"], "kept")
        self.assertEqual(new_metrics, [])

    def test_new_metric_is_labelled(self):
        merged, new_metrics = battery.merge_baselines(
            [self._row()], [], run_date="2026-01-01", branch="main", commit="abc"
        )
        self.assertEqual(len(new_metrics), 1)
        self.assertIn("new metric recorded", merged[0]["note"])

    def test_metadata_round_trips_through_the_file(self):
        record = sample_record()
        rows, identity, validity = battery._pooled_report_rows([record])
        text = battery.baselines_text(
            battery.merge_baselines(rows, [], run_date="2026-01-01", branch="main", commit="abc")[
                0
            ],
            [],
            {CELL: {"identity": identity, "validity": validity}},
        )
        path = Path(self.id().replace(".", "_") + ".tsv")
        try:
            path.write_text(text)
            metadata = battery.load_baseline_metadata(path)
            self.assertEqual(metadata[CELL]["identity"]["source"], "battery")
            self.assertEqual(metadata[CELL]["validity"]["reps"], 1)
            self.assertEqual(len(battery.load_baselines(path)), len(rows))
        finally:
            path.unlink(missing_ok=True)


class RecordingTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def _parent(self, source="battery", cell=CELL):
        parent = self.root / cell
        (parent / f"{cell}-rep1").mkdir(parents=True)
        rows, identity, validity = battery._pooled_report_rows([sample_record(cell=cell)])
        identity["source"] = source
        battery.write_report(parent / "report.tsv", [(identity, validity, rows)])
        return parent

    def test_battery_source_is_recorded(self):
        parent = self._parent()
        out = self.root / "out.tsv"
        rc = battery.record_baselines([parent], out, merge_path=self.root / "empty.tsv")
        self.assertEqual(rc, 0)
        rows = battery.load_baselines(out)
        self.assertTrue(rows)
        self.assertEqual(rows[0]["cell"], CELL)

    def test_debug_cells_are_not_recorded(self):
        parent = self._parent(source="debug")
        out = self.root / "out.tsv"
        rc = battery.record_baselines([parent], out, merge_path=self.root / "empty.tsv")
        self.assertEqual(rc, 2)
        self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
