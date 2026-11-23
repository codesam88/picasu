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


def sample_record(
    cell="control-100",
    profile="control",
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
            "probe_coverage_pct": 50.0,
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


class CellDefinitionTest(unittest.TestCase):
    def test_full_matrix_uses_profile_and_image_count(self):
        cells = battery.battery_cells(100)
        ids = [cell.cell_id for cell in cells]
        self.assertIn("control-100", ids)
        self.assertIn("storm-read-8-100", ids)
        self.assertIn("storm-idle-read-16n-100", ids)
        self.assertIn("upload-100", ids)
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(len(cells), 10)

    def test_repo_follows_image_count(self):
        cells = battery.battery_cells(5650)
        self.assertEqual({cell.repo for cell in cells}, {"set-5650"})

    def test_upload_cell_declares_its_source(self):
        cell = next(cell for cell in battery.battery_cells(100) if cell.profile == "upload")
        self.assertEqual(cell.env["UPLOAD_FILES"], str(config.DEFAULT_UPLOAD_FILES))
        self.assertEqual(cell.env["UPLOAD_REPO"], "set-upload")

    def test_storm_cell_resolves_clients(self):
        cell = next(cell for cell in battery.battery_cells(100) if cell.profile == "storm-read-16n")
        self.assertEqual(cell.env["STORM_CLIENTS"], str(16 * cell.ncpu))

    def test_list_prints_cell_ids(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = battery.main(["list", "--images", "200"])
        self.assertEqual(rc, 0)
        self.assertIn("control-200", buf.getvalue())
        self.assertNotIn("gate", buf.getvalue())


class ExtractEntryTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_profile_and_images_come_from_the_cell_name(self):
        path = self.root / "storm-read-8-100-rep2"
        path.mkdir()
        with mock.patch.object(battery, "build_row", return_value=sample_record()):
            entry = battery._extract_entry(path)
        self.assertNotIn("error", entry)
        self.assertEqual(entry["row"]["cell"], "control-100")

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
        self.cell = battery._cell("control", 100, "set-100")

    def test_missing_status_is_not_reused(self):
        self.assertIsNone(battery._reuse_row(self.cell, self.root))

    def test_completed_rep_is_reused(self):
        (self.root / "status-final.json").write_text('{"state": "completed"}')
        record = sample_record()
        with mock.patch.object(battery, "build_row", return_value=record) as build:
            row = battery._reuse_row(self.cell, self.root)
        self.assertIs(row, record)
        build.assert_called_once()
        self.assertEqual(build.call_args.kwargs["cell_id"], "control-100")

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
        self.assertEqual(identity["cell"], "control-100")
        self.assertEqual(validity["reps"], 2)

    def test_counts_stay_integers(self):
        key = metrics.metric_key("count", "scan", "prefetch")
        rows, _identity, _validity = battery._pooled_report_rows(
            [sample_record(samples={key: [0.0, 2.0]})]
        )
        self.assertEqual([row["value"] for row in rows], ["2"])
        self.assertEqual(rows[0]["unit"], "ops")


class BaselineMergeTest(unittest.TestCase):
    def _row(self, cell="control-100", subject="prefetch", stat="p50", value="7.0"):
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
            **self._row(cell="control-50"),
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
        self.assertEqual(by_cell["control-100"]["note"], "why it moved")
        self.assertEqual(by_cell["control-100"]["value"], "7.2")
        self.assertEqual(by_cell["control-100"]["recorded"], "2026-01-01 main abc1234")
        self.assertEqual(by_cell["control-50"]["note"], "kept")
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
            {"control-100": {"identity": identity, "validity": validity}},
        )
        path = Path(self.id().replace(".", "_") + ".tsv")
        try:
            path.write_text(text)
            metadata = battery.load_baseline_metadata(path)
            self.assertEqual(metadata["control-100"]["identity"]["source"], "battery")
            self.assertEqual(metadata["control-100"]["validity"]["reps"], 1)
            self.assertEqual(len(battery.load_baselines(path)), len(rows))
        finally:
            path.unlink(missing_ok=True)


class RecordingTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def _parent(self, source="battery", cell="control-100"):
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
        self.assertEqual(rows[0]["cell"], "control-100")

    def test_debug_cells_are_not_recorded(self):
        parent = self._parent(source="debug")
        out = self.root / "out.tsv"
        rc = battery.record_baselines([parent], out, merge_path=self.root / "empty.tsv")
        self.assertEqual(rc, 2)
        self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
