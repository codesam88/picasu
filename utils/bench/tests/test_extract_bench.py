#!/usr/bin/env python3
"""Tests for extract_bench.py: the v1 event reader and its transcript fallback.

`fixtures/events-dual/` holds two cells recorded by the gate battery
(`gate-upload-rep2` and `gate-storm-diff-rep1`, 2026-11-15). Each cell ships
the recorded `harness-a.log`, `status-final.json` and `scan-walls.tsv`; the
control cell also holds an excerpt of the recorded `server.log` with its two
`observe drain:` tables, and both cells hold the recorded per-round TSVs
(`rounds-run-*/`) that the counts come from. `run-manifest.json` keeps the
recorded identity values under the keys the extractor and the battery read.

`events.jsonl` is derived, not recorded: the cells predate the stream, so
each line is a replay of the cell's own artifacts — per-file `upload_file`
and per-run `scan_state` from the transcript, fetch means from the probe
TSVs, per-op `storm_sample` from the storm TSVs, summaries over the same
TSVs — with the full-precision values `workload.emit_event` writes. Samples
are grouped per run instead of interleaved as they were emitted, and their
timestamps are order placeholders. `tests/test_recorded_fixtures.py`
re-derives the stream from those artifacts; `fixtures/README.md` records
each file's provenance. This pair is what proves the event path reports the
numbers the transcript records.
"""

import contextlib
import io
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))
FIXTURES = Path(__file__).resolve().parent / "fixtures" / "events-dual"
CELLS = ("control", "storm-diff")

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
                profile="storm-diff",
                kind="diff",
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
                profile="storm-diff",
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


class FixtureCase(unittest.TestCase):
    """Helpers: a writable copy of a recorded dual fixture cell."""

    def cell(self, name: str, *, events: bool = True) -> Path:
        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, tmp, ignore_errors=True)
        target = tmp / name
        shutil.copytree(FIXTURES / name, target)
        if not events:
            (target / "events.jsonl").unlink()
        return target

    def rewrite_events(self, cell: Path, mutate) -> None:
        path = cell / "events.jsonl"
        lines = [json.loads(line) for line in path.read_text().splitlines()]
        path.write_text("".join(json.dumps(mutate(event)) + "\n" for event in lines))


_VALIDITY_KEYS = {
    "probe_rounds_total",
    "probe_rounds_valid",
    "probe_coverage_pct",
    "warmup_valid",
    "phase_invalid",
    "phase_invalid_reasons",
    "storm_coverage_pct",
}


def _strip_validity(row):
    return {key: value for key, value in row.items() if key not in _VALIDITY_KEYS}


class FallbackTest(FixtureCase):
    pass


class PreferenceTest(FixtureCase):
    def test_measurements_follow_the_event_stream_not_the_transcript(self):
        cell = self.cell("control")
        legacy = extractor().parse_log(self.cell("control", events=False))

        def mutate(event):
            if event["type"] == "probe_round" and "fetch_n" in event:
                event["fetch_mean"] = 0.5
            if event["type"] == "scan_state":
                event["wall_ms"] = 40000
            return event

        self.rewrite_events(cell, mutate)
        row = extractor().parse_log(cell)
        self.assertGreater(row["scan_thumb_mean"], 0.4)
        self.assertNotEqual(row["scan_thumb_mean"], legacy["scan_thumb_mean"])
        self.assertEqual(row["scan_runs_ms"], [40.0, 40.0])
        self.assertEqual(legacy["scan_runs_ms"], [18.556, 18.776])

    def test_storm_aggregates_follow_the_event_stream(self):
        cell = self.cell("storm-diff")
        legacy = extractor().parse_log(self.cell("storm-diff", events=False))

        def mutate(event):
            if event["type"] == "storm_summary":
                event["median"] = 0.111
            return event

        self.rewrite_events(cell, mutate)
        row = extractor().parse_log(cell)
        self.assertEqual([storm["median"] for storm in row["storms"]], [0.111, 0.111])
        self.assertNotEqual(
            [storm["median"] for storm in row["storms"]],
            [storm["median"] for storm in legacy["storms"]],
        )


class DualFixtureGateTest(FixtureCase):
    """The gate: a cell with events.jsonl extracts exactly like its log."""

    def _run(self, cell: Path):
        from types import SimpleNamespace

        out, err = io.StringIO(), io.StringIO()
        with (
            contextlib.redirect_stdout(out),
            contextlib.redirect_stderr(err),
        ):
            try:
                extractor().main([str(cell)])
            except SystemExit as exc:
                rc = exc.code if isinstance(exc.code, int) else 1
            else:
                rc = 0
        return SimpleNamespace(stdout=out.getvalue(), stderr=err.getvalue(), returncode=rc)


class MatrixStormTest(FixtureCase):
    """Storm bursts land in their background and kind, failures included."""


class MatrixProbeBucketTest(FixtureCase):
    """Upload probes are `uscan`, every other profile's are `scan`."""


class MatrixIdleTailTest(FixtureCase):
    """Idle p50 pools the timed runs; p95/max pool the same runs."""

    def _with_export(self, results) -> Path:
        cell = self.cell("control")
        (cell / "hyperfine.json").write_text(json.dumps({"results": results}))
        return cell


if __name__ == "__main__":
    unittest.main()


class TranscriptFormatTest(unittest.TestCase):
    """A new-format cell (slim log + per-round artifacts, no inline
    numbers) extracts identically with and without its event stream."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.cell = self.tmp / "cell"
        rounds = self.cell / "rounds-run-1"
        rounds.mkdir(parents=True)
        (self.cell / "harness-a.log").write_text(
            "\n".join(
                [
                    "02:40:57 N=8 image_count=100 image_bytes=54272755",
                    "02:40:57 === seeding images (repo=/tmp/sets/set-100 path=/tmp/x) ===",
                    "02:40:57 scan run 1/2: state=running polls=12 wall_ms=10000",
                    "02:41:16 scan run 2/2: state=completed polls=3 wall_ms=12000",
                    "02:41:16 scan wall median_ms=11000 over 2 run(s): 10000,12000",
                    "02:41:16 probe round=1 rows=100 state=running",
                    "02:41:17 probe round=2 rows=100 state=running",
                    "02:41:18 storm: profile=storm-diff ops=4/4 failed=1 wall=0.1s kind=diff",
                    "02:41:18 storm verify: matched 8/8 canary writes",
                ]
            )
            + "\n"
        )
        (self.cell / "status-final.json").write_text('{"state": "completed"}')
        (self.cell / "scan-walls.tsv").write_text("1\t10000\n2\t12000\n")
        (rounds / "hyperfine-round-1.json").write_text(
            json.dumps(
                {
                    "results": [
                        {"name": "prefetch", "mean": 0.0081, "median": 0.008},
                        {"name": "edit-rating", "mean": 0.0111, "median": 0.011},
                    ]
                }
            )
        )
        (rounds / "hyperfine-round-2.json").write_text(
            json.dumps(
                {
                    "results": [
                        {"name": "prefetch", "mean": 0.0091, "median": 0.009},
                        {"name": "edit-rating", "mean": 0.0121, "median": 0.012},
                    ]
                }
            )
        )
        (rounds / "probe-round-1.tsv").write_text("h1\t200\t0.004\nh2\t200\t0.008\n")
        (rounds / "probe-round-2.tsv").write_text("h3\t200\t0.010\n")
        (rounds / "storm-round-1.tsv").write_text(
            "0\t0\t0.003\t200\n1\t1\t0.004\t200\n2\t0\t0.005\terror\n3\t1\t0.006\t200\n"
        )
        (self.cell / "hyperfine.json").write_text(
            json.dumps(
                {
                    "results": [
                        {"name": "compressed-get", "median": 0.007},
                        {"name": "prefetch", "median": 0.008},
                        {"name": "edit-rating", "median": 0.009},
                    ]
                }
            )
        )
        events = [
            {
                "v": 1,
                "type": "probe_round",
                "round": 1,
                "rows": 100,
                "fetch_n": 2,
                "fetch_mean": 0.006,
            },
            {
                "v": 1,
                "type": "probe_round",
                "round": 1,
                "rows": 100,
                "prefetch_median": 0.008,
                "edit_median": 0.011,
            },
            {
                "v": 1,
                "type": "probe_round",
                "round": 2,
                "rows": 100,
                "fetch_n": 1,
                "fetch_mean": 0.01,
            },
            {
                "v": 1,
                "type": "probe_round",
                "round": 2,
                "rows": 100,
                "prefetch_median": 0.009,
                "edit_median": 0.012,
            },
            {"v": 1, "type": "scan_state", "run": 1, "state": "running", "wall_ms": 10000},
            {"v": 1, "type": "scan_state", "run": 2, "state": "completed", "wall_ms": 12000},
            {
                "v": 1,
                "type": "storm_summary",
                "profile": "storm-diff",
                "kind": "diff",
                "done": 4,
                "total": 4,
                "median": 0.0045000000000000005,
                "p95": 0.00585,
                "max": 0.006,
                "failed": 1,
                "wall": 0.1,
            },
            {"v": 1, "type": "verify_result", "kind": "storm", "matched": 8, "total": 8},
        ]
        (self.cell / "events.jsonl").write_text("".join(json.dumps(e) + "\n" for e in events))

    def test_slim_log_parses_without_events(self):
        (self.cell / "events.jsonl").unlink()
        row = extractor().parse_log(self.cell)
        self.assertEqual(row["probe_rounds"], 2)
        self.assertEqual(row["scan_rounds"], 2)
        self.assertEqual(row["scan_thumb_n"], 3)
        self.assertEqual(row["scan_wall_s"], 11.0)
        self.assertEqual(row["scan_runs"], 2)
        self.assertEqual(row["post_compressed"], 0.007)
        self.assertEqual(row["post_prefetch"], 0.008)
        self.assertEqual(row["post_edit"], 0.009)
        self.assertEqual(row["storms"][0]["done"], 4)
        self.assertEqual(row["storms"][0]["failed"], 1)
        self.assertEqual(row["storm_verify"], [(8, 8)])


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
        shutil.copytree(FIXTURES / "control", directory / "control")
        target = directory / "control"
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

    def test_warmup_false(self):
        stream = v1(type="warmup", valid=False, reason="no probe rows", wall_ms=5)
        events = self._events(stream)
        row = {"benchmark": "cell"}
        self.extract._apply_events(row, events, "")
        self.assertEqual(row["warmup_valid"], "false")

    def test_coverage_math_cap(self):
        stream = v1(
            type="phase_guard", phase="probe", valid=True, wall_ms=1000, run=1, round=1
        ) + v1(type="phase_guard", phase="probe", valid=True, wall_ms=1000, run=1, round=2)
        events = self._events(stream)
        row = {"benchmark": "cell", "scan_wall_s": 0.01, "scan_runs": 1}
        self.extract._apply_events(row, events, "")
        self.assertEqual(row["probe_coverage_pct"], 100.0)

    def test_old_cell_without_events_is_na(self):
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp) / "cell"
            cell.mkdir()
            (cell / "harness-a.log").write_text("image_count=1 image_bytes=1\n")
            row = self.extract.parse_log(cell)
            self.assertEqual(row.get("probe_rounds_total"), "n/a")
            self.assertEqual(row.get("warmup_valid"), "n/a")
            self.assertEqual(row.get("phase_invalid_reasons"), "n/a")


class StormKindAliasTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()


class StormBackgroundTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()

    def test_storm_background_scan(self):
        self.assertEqual(self.extract.storm_background({"profile": "storm-edit-16n"}), "scan")


class StormKindExtraTest(unittest.TestCase):
    def setUp(self):
        self.extract = extractor()

    def test_storm_kind_thumb_8(self):
        self.assertEqual(self.extract.storm_kind({"profile": "storm-thumb-8"}), "thumb")


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

    def test_old_cell_storm_coverage_na(self):
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp) / "cell"
            cell.mkdir()
            (cell / "harness-a.log").write_text("image_count=1 image_bytes=1\n")
            row = self.extract.parse_log(cell)
            self.assertEqual(row.get("storm_coverage_pct"), "n/a")


class RawSampleTest(unittest.TestCase):
    """Every individual measurement, in seconds, behind p50 and CI."""

    def setUp(self):
        self.extract = extractor()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.cell = Path(self.tmp.name) / "cell"
        self.cell.mkdir()
        (self.cell / "harness-a.log").write_text(
            "scan wall median_ms=12 over 2 run(s): 11,13\n"
            "upload file=u1.jpg submit_ms=10 visible_ms=50\n"
            "upload file=u2.jpg submit_ms=14 visible_ms=60\n"
            "upload file=u3.jpg submit_ms=18 visible_ms=70\n"
            "upload submit_p50=14 visible_p50=60\n"
            "storm: profile=storm-read-8 ops=3/3 failed=0 wall=1.0s\n"
        )
        (self.cell / "hyperfine.json").write_text(
            json.dumps(
                {
                    "results": [
                        {"name": "prefetch", "times": [0.010, 0.020, 0.030]},
                        {"name": "compressed-get", "times": [0.004, 0.006]},
                        {"name": "edit-rating", "times": [0.040, 0.050]},
                    ]
                }
            )
        )
        for run, prefetch, edit in (
            (1, [0.010, 0.020], [0.040, 0.050, 0.060]),
            (2, [0.030], [0.070]),
        ):
            rounds = self.cell / f"rounds-run-{run}"
            rounds.mkdir(exist_ok=True)
            (rounds / "hyperfine-round-1.json").write_text(
                json.dumps(
                    {
                        "results": [
                            {"name": "prefetch", "median": 0.015, "times": prefetch},
                            {"name": "edit-rating", "median": 0.05, "times": edit},
                        ]
                    }
                )
            )
        (self.cell / "rounds-run-1" / "probe-round-1.tsv").write_text(
            "h1\t200\t0.001\nh2\t200\t0.002\nh3\t200\t0.003\n"
        )
        (self.cell / "rounds-run-2" / "probe-round-1.tsv").write_text("h4\t200\t0.009\n")
        (self.cell / "rounds-run-1" / "storm-round-1.tsv").write_text(
            "1\t0\t0.005\t200\n2\t0\t0.010\t200\n3\t0\t0.015\t200\n"
        )
        (self.cell / "rounds-run-2" / "storm-round-1.tsv").write_text("4\t0\t0.020\t200\n")

    def parsed(self, profile="storm-read-8"):
        return self.extract.parse_log(self.cell, profile=profile)

    def test_idle_runs_are_raw_samples(self):
        row = self.parsed("control")
        self.assertEqual(row["raw_samples"]["idle_prefetch"], [0.010, 0.020, 0.030])
        self.assertEqual(row["raw_samples"]["idle_thumb"], [0.004, 0.006])
        self.assertEqual(row["raw_samples"]["idle_edit"], [0.040, 0.050])
        self.assertEqual(row["raw_cluster_counts"]["idle_prefetch"], 3)

    def test_probe_runs_are_raw_samples_with_round_clusters(self):
        row = self.parsed("control")
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

    def test_storm_ops_are_seconds_with_run_clusters(self):
        row = self.parsed("storm-read-8")
        self.assertEqual(row["raw_samples"]["scan_storm_prefetch"], [0.005, 0.010, 0.015, 0.020])
        self.assertEqual(row["raw_cluster_counts"]["scan_storm_prefetch"], 2)

    def test_upload_files_are_raw_samples_from_the_log(self):
        row = self.parsed("control")
        self.assertEqual(row["raw_samples"]["upload_submit"], [0.010, 0.014, 0.018])
        self.assertEqual(row["raw_samples"]["upload_visible"], [0.050, 0.060, 0.070])
        self.assertEqual(row["raw_cluster_counts"]["upload_submit"], 3)

    def test_scan_walls_are_raw_samples_with_scan_clusters(self):
        row = self.parsed("control")
        self.assertEqual(row["raw_samples"]["scan_wall"], [0.011, 0.013])
        self.assertEqual(row["raw_cluster_counts"]["scan_wall"], 2)

    def test_bad_artifact_rows_are_skipped(self):
        (self.cell / "rounds-run-1" / "probe-round-1.tsv").write_text(
            "h1\t200\t0.001\nh2\t200\tjunk\nh3\t200\n"
        )
        row = self.parsed("control")
        self.assertEqual(row["raw_samples"]["scan_thumb"], [0.001, 0.009])
        self.assertEqual(row["raw_cluster_counts"]["scan_thumb"], 2)

    def test_event_path_uses_event_values_for_upload_and_storm(self):
        events = self.extract.Events(
            upload_files=[
                {"file": "a", "submit_ms": 10, "visible_ms": 50},
                {"file": "b", "submit_ms": 20, "visible_ms": 70},
            ],
            storm_samples=[
                {"profile": "storm-edit-8", "latency_ms": 4, "code": 200},
                {"profile": "storm-edit-8", "latency_ms": 8, "code": 200},
                {"profile": "storm-idle-8", "latency_ms": 12, "code": 200},
            ],
            storm_summaries=[{"profile": "storm-edit-8", "median": 0.006}],
        )
        row = {"scan_runs_ms": [11.0]}
        self.extract._collect_raw_samples(row, self.cell, "storm-edit-8", events)

        self.assertEqual(row["raw_samples"]["upload_submit"], [0.010, 0.020])
        self.assertEqual(row["raw_cluster_counts"]["upload_submit"], 2)
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
