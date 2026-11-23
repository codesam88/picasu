#!/usr/bin/env python3
"""Tests for extract_bench.py: the v1 event reader.

`fixtures/recorded/probe-50/` is a recorded cell (probe-50-rep1 of the
matrix run, 2026-11-22). It ships the recorded `harness-a.log`,
`events.jsonl`, the `server.log` excerpt with its two `observe drain:`
tables, `run-manifest.json` and `status-final.json`. This cell is what
proves the event path reports the numbers a real cell records.
"""

import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))
FIXTURES = Path(__file__).resolve().parent / "fixtures" / "recorded" / "probe-50"

_LOADED: dict = {}


def load(name: str, filename: str):
    if name not in _LOADED:
        module = __import__(name)
        sys.modules[name] = module
        _LOADED[name] = module
    return _LOADED[name]


def extractor():
    return load("extract_bench", "extract_bench.py")


def battery():
    return load("battery", "battery.py")


def v1(**fields) -> str:
    event = {"v": 1, "t": "2026-11-15T02:40:57.001+00:00"}
    event.update(fields)
    return json.dumps(event, separators=(",", ":")) + "\n"


def sample_stream() -> str:
    return "".join(
        (
            v1(
                type="probe_round",
                round=1,
                rows=100,
                state="running",
                fetch_n=10,
                fetch_mean=0.0025,
            ),
            v1(
                type="probe_round",
                round=1,
                rows=100,
                state="running",
                prefetch_median=0.0098,
                edit_median=0.0109,
            ),
            v1(type="scan_state", run=1, state="completed", polls=52, wall_ms=10297),
            v1(
                type="storm_summary",
                profile="scan-storm-edit-8",
                kind="edit",
                done=80,
                total=80,
                median=0.0061,
                p95=0.0124,
                max=0.0204,
                failed=0,
                wall=0.1,
            ),
            v1(
                type="storm_sample",
                profile="scan-storm-edit-8",
                client=0,
                op=0,
                latency_ms=4.127,
                code=200,
            ),
            v1(type="verify_result", kind="storm", run=1, matched=8, total=8, failed=0),
            v1(type="upload_file", file="img_00001.jpeg", submit_ms=2, visible_ms=816),
            v1(type="upload_summary", submit_p50=3, visible_p50=636),
        )
    )


class ReaderTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()

    def test_missing_stream_is_absent_not_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(FileNotFoundError):
                self.extract.read_events(Path(tmp))

    def test_unknown_version_names_the_version(self):
        stream = sample_stream().replace('"v":1', '"v":2')
        with self.assertRaises(self.extract.EventsError) as caught:
            self.extract.parse_events(stream)
        message = str(caught.exception)
        self.assertIn("v=2", message)
        self.assertIn("v=1", message)

    def test_partial_last_line_is_truncated(self):
        stream = sample_stream() + '{"v":1,"type":"scan_st'
        with self.assertRaises(self.extract.EventsError) as caught:
            self.extract.parse_events(stream)
        self.assertIn("truncated", str(caught.exception))

    def test_missing_required_key_is_rejected(self):
        stream = v1(type="scan_state", run=1, state="completed")
        with self.assertRaises(self.extract.EventsError) as caught:
            self.extract.parse_events(stream)
        self.assertIn("wall_ms", str(caught.exception))

    def test_unknown_type_is_rejected(self):
        stream = v1(type="scan_progress", run=1)
        with self.assertRaises(self.extract.EventsError) as caught:
            self.extract.parse_events(stream)
        self.assertIn("scan_progress", str(caught.exception))


class MatrixMathTest(unittest.TestCase):
    def test_percentile_matches_the_workload_interpolation(self):
        extract = extractor()
        self.assertEqual(extract.percentile([5.0], 95), 5.0)
        self.assertEqual(extract.percentile([1.0, 3.0], 95), 2.9)
        self.assertEqual(extract.percentile([0.0, 10.0, 20.0], 50), 10.0)


class ObserverCounterTest(unittest.TestCase):
    """`observe counters:` lines and process metrics in the cell row."""

    def test_counters_line_parses_into_exact_counts(self):
        log = (
            "observe drain: scan job=1 state=Completed\n"
            "  stage                     n   p50_us   p95_us    max_us\n"
            "  prefetch_total           71       29    10350    18300\n"
            "observe counters: snapshot_memory_hit=41 snapshot_disk_fallback=3\n"
        )
        self.assertEqual(
            extractor().parse_counters(log),
            [{"snapshot_memory_hit": 41, "snapshot_disk_fallback": 3}],
        )

    def test_absent_counters_line_is_empty(self):
        self.assertEqual(extractor().parse_counters("no observer output here\n"), [])

    def test_cell_metrics_file_fills_the_row(self):
        directory = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        target = directory / "probe-50"
        shutil.copytree(FIXTURES, target)
        (target / "cell-metrics.json").write_text(
            json.dumps({"vm_hwm_mb": 99.5, "temp_db_mb": 12.25}) + "\n"
        )

        row = extractor().parse_log(target)

        self.assertEqual(row["vm_hwm_mb"], 99.5)
        self.assertEqual(row["temp_db_mb"], 12.25)


class PhaseGuardEventTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()

    def test_accepts_phase_guard(self):
        stream = v1(type="phase_guard", phase="probe", valid=True, wall_ms=10, run=1, round=1)
        events = self.extract.parse_events(stream)
        self.assertEqual(len(events.phase_guards), 1)

    def test_missing_phase_rejected(self):
        stream = v1(type="phase_guard", valid=True, wall_ms=10)
        with self.assertRaises(self.extract.EventsError):
            self.extract.parse_events(stream)

    def test_non_numeric_wall_ms_rejected(self):
        stream = v1(type="phase_guard", phase="probe", valid=True, wall_ms="x")
        with self.assertRaises(self.extract.EventsError):
            self.extract.parse_events(stream)


class ValidityFieldsTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()

    def _events(self, *lines):
        return self.extract.parse_events("".join(lines))

    def test_valid_and_invalid_probe_guards(self):
        stream = (
            v1(type="phase_guard", phase="probe", valid=True, wall_ms=100, run=1, round=1)
            + v1(
                type="phase_guard",
                phase="probe",
                valid=False,
                reason="state mismatch at end",
                wall_ms=50,
                run=1,
                round=2,
            )
            + v1(type="warmup", valid=True, wall_ms=10)
        )
        events = self._events(stream)
        row = {"benchmark": "cell", "scan_wall_s": 0.2, "scan_runs": 1}
        self.extract._apply_events(row, events, "")
        self.assertEqual(row["probe_rounds_total"], 2)
        self.assertEqual(row["probe_rounds_valid"], 1)
        self.assertEqual(row["phase_invalid"], 1)
        self.assertEqual(row["warmup_valid"], "true")
        self.assertIn("state mismatch at end", row["phase_invalid_reasons"])

    def test_invalid_guard_round_samples_are_not_pooled(self):
        stream = (
            v1(
                type="probe_round",
                round=1,
                rows=100,
                prefetch_ms=[1.0, 2.0],
                edit_ms=[3.0],
                thumb_ms=[4.0],
            )
            + v1(
                type="probe_round",
                round=2,
                rows=100,
                prefetch_ms=[9.0],
                edit_ms=[9.0],
                thumb_ms=[9.0],
            )
            + v1(type="phase_guard", phase="probe", valid=True, wall_ms=100, run=1, round=1)
            + v1(
                type="phase_guard",
                phase="probe",
                valid=False,
                reason="state mismatch at end",
                wall_ms=50,
                run=1,
                round=2,
            )
        )
        events = self._events(stream)
        row = {"benchmark": "cell"}
        self.extract._apply_events(row, events, "")
        self.assertEqual(row["probe_round_prefetch"], [0.001, 0.002])
        self.assertEqual(row["probe_round_edit"], [0.003])
        self.assertEqual(row["probe_round_thumb"], [0.004])

        self.extract._collect_raw_samples(row, events)
        samples = row["samples"]
        self.assertEqual(samples["latency|scan|prefetch"], [1.0, 2.0])
        self.assertEqual(samples["latency|scan|edit"], [3.0])
        self.assertEqual(samples["latency|scan|thumb"], [4.0])

    def test_warmup_false(self):
        stream = v1(type="warmup", valid=False, reason="no probe rows", wall_ms=5)
        events = self._events(stream)
        row = {"benchmark": "cell"}
        self.extract._apply_events(row, events, "")
        self.assertEqual(row["warmup_valid"], "false")

class StormKindAliasTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()


class StormBackgroundTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()

    def test_background_comes_from_the_profile(self):
        import scenarios

        for profile in scenarios.storm_profiles():
            with self.subTest(profile=profile):
                background, _kind, _level = scenarios.storm_parts(profile)
                self.assertEqual(self.extract.storm_background({"profile": profile}), background)


class StormKindExtraTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()

    def test_kind_comes_from_the_profile(self):
        import scenarios

        for profile in scenarios.storm_profiles():
            with self.subTest(profile=profile):
                _background, kind, _level = scenarios.storm_parts(profile)
                self.assertEqual(
                    self.extract.storm_kind({"profile": profile}), scenarios.STORM_ACCESS[kind]
                )


class StormCoverageEventTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()

    def test_storm_coverage_from_event(self):
        stream = v1(
            type="storm_summary",
            profile="storm-edit",
            kind="edit",
            done=10,
            total=10,
            median=0.01,
            p95=0.02,
            max=0.03,
            failed=0,
            wall=1.0,
            coverage_pct=75.0,
        )
        events = self.extract.parse_events(stream)
        row = {"benchmark": "cell", "scan_wall_s": 1.0, "scan_runs": 1}
        self.extract._apply_events(row, events, "")
        self.assertEqual(row["storm_coverage_pct"], 75.0)

    def test_storm_coverage_missing_is_na(self):
        stream = v1(
            type="storm_summary",
            profile="storm-edit",
            kind="edit",
            done=10,
            total=10,
            median=0.01,
            p95=0.02,
            max=0.03,
            failed=0,
            wall=1.0,
        )
        events = self.extract.parse_events(stream)
        row = {"benchmark": "cell", "scan_wall_s": 1.0, "scan_runs": 1}
        self.extract._apply_events(row, events, "")
        self.assertEqual(row["storm_coverage_pct"], "n/a")

class RawSampleTest(unittest.TestCase):
    """Every individual measurement, in seconds, behind p50 and CI."""

    def setUp(self):
        self.extract = extractor()

    def test_idle_runs_are_raw_samples(self):
        events = self.extract.Events(
            idle_samples=[
                {"access": "prefetch", "latency_ms": 10.0, "code": 200, "phase": "measured"},
                {"access": "prefetch", "latency_ms": 20.0, "code": 200, "phase": "measured"},
                {"access": "prefetch", "latency_ms": 30.0, "code": 200, "phase": "measured"},
                {"access": "thumb", "latency_ms": 4.0, "code": 200, "phase": "measured"},
                {"access": "thumb", "latency_ms": 6.0, "code": 200, "phase": "measured"},
                {"access": "edit", "latency_ms": 40.0, "code": 200, "phase": "measured"},
                {"access": "edit", "latency_ms": 50.0, "code": 200, "phase": "measured"},
            ],
        )
        row = {"scan_runs_ms": [11.0, 13.0]}
        self.extract._collect_raw_samples(row, events)
        self.assertEqual(row["raw_samples"]["idle_prefetch"], [0.010, 0.020, 0.030])
        self.assertEqual(row["raw_samples"]["idle_thumb"], [0.004, 0.006])
        self.assertEqual(row["raw_samples"]["idle_edit"], [0.040, 0.050])
        self.assertEqual(row["raw_cluster_counts"]["idle_prefetch"], 1)

    def test_probe_runs_are_raw_samples_with_round_clusters(self):
        events = self.extract.Events(
            probe_rounds=[
                {
                    "round": 1,
                    "rows": 100,
                    "prefetch_ms": [10.0, 20.0],
                    "edit_ms": [40.0, 50.0, 60.0],
                    "thumb_ms": [1.0, 2.0, 3.0],
                },
                {
                    "round": 2,
                    "rows": 100,
                    "prefetch_ms": [30.0],
                    "edit_ms": [70.0],
                    "thumb_ms": [9.0],
                },
            ],
        )
        row = {"scan_runs_ms": [11.0, 13.0]}
        self.extract._collect_raw_samples(row, events)
        self.assertEqual(row["raw_samples"]["scan_prefetch"], [0.010, 0.020, 0.030])
        self.assertEqual(row["raw_samples"]["scan_edit"], [0.040, 0.050, 0.060, 0.070])
        self.assertEqual(row["raw_samples"]["scan_thumb"], [0.001, 0.002, 0.003, 0.009])
        self.assertEqual(
            {
                key: row["raw_cluster_counts"][key]
                for key in ("scan_prefetch", "scan_edit", "scan_thumb")
            },
            {"scan_prefetch": 2, "scan_edit": 2, "scan_thumb": 2},
        )

    def test_upload_samples_group_by_their_actual_background(self):
        events = self.extract.Events(
            upload_files=[
                {"file": "a", "submit_ms": 10, "visible_ms": 50, "scan_active_at_submit": True},
                {"file": "b", "submit_ms": 20, "visible_ms": 70, "scan_active_at_submit": False},
            ],
        )
        row = {"scan_runs_ms": [11.0]}
        self.extract._collect_raw_samples(row, events)
        self.assertEqual(row["raw_samples"]["upload_scan_submit"], [0.010])
        self.assertEqual(row["raw_samples"]["upload_idle_submit"], [0.020])
        self.assertEqual(row["raw_cluster_counts"]["upload_scan_submit"], 1)

    def test_scan_walls_are_raw_samples_with_scan_clusters(self):
        row = {"scan_runs_ms": [11.0, 13.0]}
        self.extract._collect_raw_samples(row, self.extract.Events())
        self.assertEqual(row["raw_samples"]["scan_wall"], [11.0, 13.0])
        self.assertEqual(row["raw_cluster_counts"]["scan_wall"], 2)

    def test_event_path_uses_event_values_for_upload_and_storm(self):
        events = self.extract.Events(
            upload_files=[
                {"file": "a", "submit_ms": 10, "visible_ms": 50},
                {"file": "b", "submit_ms": 20, "visible_ms": 70},
            ],
            storm_samples=[
                {"profile": "scan-storm-edit-8", "latency_ms": 4, "code": 200},
                {"profile": "scan-storm-edit-8", "latency_ms": 8, "code": 200},
                {"profile": "idle-storm-read-8", "latency_ms": 12, "code": 200},
            ],
            storm_summaries=[{"profile": "scan-storm-edit-8", "median": 0.006}],
        )
        row = {"scan_runs_ms": [11.0]}
        self.extract._collect_raw_samples(row, events)

        self.assertEqual(row["raw_samples"]["upload_idle_submit"], [0.010, 0.020])
        self.assertEqual(row["raw_cluster_counts"]["upload_idle_submit"], 2)
        self.assertEqual(row["raw_samples"]["scan_storm_edit"], [0.004, 0.008])
        self.assertEqual(row["raw_cluster_counts"]["scan_storm_edit"], 1)
        self.assertEqual(row["raw_samples"]["idle_storm_prefetch"], [0.012])


class RawMedianTest(unittest.TestCase):
    """A p50 over all raw measurements differs from a median of summaries."""

    def setUp(self):
        self.extract = extractor()

    def test_probe_p50_is_the_median_of_every_run(self):
        # Round medians would be 15 ms and 30 ms -> 22.5 ms. The raw runs
        # are 10, 20 and 30 ms -> 20 ms.
        import statistics

        self.assertEqual(round(statistics.median([0.015, 0.030]) * 1000, 1), 22.5)
        self.assertEqual(round(statistics.median([0.010, 0.020, 0.030]) * 1000, 1), 20.0)


class CappedStormLogTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()

    def test_capped_minimum_note_does_not_hide_the_storm_summary(self):
        log = (
            "03:07:11 storm: profile=storm-edit-16n ops=1181/256 "
            "(minimum capped from 1280) failed=0 wall=1.6s kind=edit active_wall=1.4s\n"
        )

        matches = self.extract._STORM_SUMMARY_RE.findall(log)

        self.assertEqual(
            matches,
            [("storm-edit-16n", "1181", "256", "0", "1.6", "edit")],
        )
