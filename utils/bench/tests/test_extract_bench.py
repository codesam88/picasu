#!/usr/bin/env python3
"""Tests for extract-bench.py: the v1 event reader and its transcript fallback.

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
import importlib.util
import io
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
FIXTURES = Path(__file__).resolve().parent / "fixtures" / "events-dual"
CELLS = ("control", "storm-diff")

_LOADED: dict = {}


def load(name: str, filename: str):
    if name not in _LOADED:
        spec = importlib.util.spec_from_file_location(name, BENCH / filename)
        module = importlib.util.module_from_spec(spec)
        sys.modules[name] = module
        spec.loader.exec_module(module)
        _LOADED[name] = module
    return _LOADED[name]


def extractor():
    return load("extract_bench", "extract-bench.py")


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

    def test_v1_stream_becomes_typed_lists(self):
        events = self.extract.read_events(self._with_stream(sample_stream()))
        self.assertEqual(len(events.probe_rounds), 2)
        self.assertEqual(events.probe_rounds[0]["fetch_n"], 10)
        self.assertEqual(events.scan_states[0]["wall_ms"], 10297)
        self.assertEqual(events.storm_summaries[0]["kind"], "diff")
        self.assertEqual(len(events.storm_samples), 1)
        self.assertEqual(events.verify_results[0]["matched"], 8)
        self.assertEqual(events.upload_files[0]["file"], "img_00001.jpeg")
        self.assertEqual(events.upload_summaries[0]["visible_p50"], 636)

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

    def _with_stream(self, stream: str) -> Path:
        directory = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        (Path(directory) / "events.jsonl").write_text(stream)
        return Path(directory)


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


class FallbackTest(FixtureCase):
    def test_absent_stream_reads_the_same_row_as_the_event_stream(self):
        for name in CELLS:
            with self.subTest(cell=name):
                events_row = extractor().parse_log(self.cell(name))
                legacy_row = extractor().parse_log(self.cell(name, events=False))
                self.assertEqual(legacy_row, events_row)

    def test_partial_last_line_falls_back_with_a_warning(self):
        for name in CELLS:
            with self.subTest(cell=name):
                events_row = extractor().parse_log(self.cell(name))
                cell = self.cell(name)
                text = (cell / "events.jsonl").read_text()
                (cell / "events.jsonl").write_text(text[: len(text) // 2])
                stderr = io.StringIO()
                with contextlib.redirect_stderr(stderr):
                    row = extractor().parse_log(cell)
                self.assertEqual(row, events_row)
                self.assertIn("falling back to log lines", stderr.getvalue())

    def test_stream_truncated_at_a_line_boundary_falls_back(self):
        # A cell killed mid-run leaves whole lines only: the stream parses,
        # but its counts fall short of the transcript, so it is incomplete.
        for name in CELLS:
            with self.subTest(cell=name):
                events_row = extractor().parse_log(self.cell(name))
                cell = self.cell(name)
                lines = (cell / "events.jsonl").read_text().splitlines(keepends=True)
                (cell / "events.jsonl").write_text("".join(lines[:-2]))
                stderr = io.StringIO()
                with contextlib.redirect_stderr(stderr):
                    row = extractor().parse_log(cell)
                self.assertEqual(row, events_row)
                self.assertIn("incomplete", stderr.getvalue())

    def test_unreadable_stream_falls_back_with_a_warning(self):
        cell = self.cell("control")
        events_row = extractor().parse_log(self.cell("control"))
        (cell / "events.jsonl").write_bytes(b"\xff\xfe\x00garbage")
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            row = extractor().parse_log(cell)
        self.assertEqual(row, events_row)
        self.assertIn("falling back to log lines", stderr.getvalue())

    def test_unknown_version_falls_back_with_a_warning(self):
        for name in CELLS:
            with self.subTest(cell=name):
                events_row = extractor().parse_log(self.cell(name))
                cell = self.cell(name)
                self.rewrite_events(cell, lambda event: {**event, "v": 2})
                stderr = io.StringIO()
                with contextlib.redirect_stderr(stderr):
                    row = extractor().parse_log(cell)
                self.assertEqual(row, events_row)
                self.assertIn("v=2", stderr.getvalue())
                self.assertIn("falling back to log lines", stderr.getvalue())


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

    def test_table_stages_and_battery_rows_are_byte_identical(self):
        for name in CELLS:
            with self.subTest(cell=name):
                with_events = self.cell(name)
                without = self.cell(name, events=False)
                first = self._run(with_events)
                second = self._run(without)
                self.assertEqual(first.stdout, second.stdout)
                self.assertNotIn("falling back", first.stderr)
                self.assertEqual(
                    (with_events / "stages.json").read_bytes(),
                    (without / "stages.json").read_bytes(),
                )
                row_events = battery().build_row("gate", name, with_events)
                row_legacy = battery().build_row("gate", name, without)
                self.assertEqual(row_events, row_legacy)

    def test_rebuilt_row_carries_the_whole_matrix(self):
        for name in CELLS:
            with self.subTest(cell=name):
                row = battery().build_row("gate", name, self.cell(name))
                for column in battery().LATENCY_COLUMNS + battery().STORM_FAILED_COLUMNS:
                    self.assertIn(column, row)

    def _run(self, cell: Path):
        return subprocess.run(
            [sys.executable, str(BENCH / "extract-bench.py"), str(cell)],
            capture_output=True,
            text=True,
            check=False,
        )


class MatrixSchemaTest(unittest.TestCase):
    """The matrix constants are the schema every consumer agrees on."""

    def test_every_background_access_and_stat_has_a_column(self):
        extract = extractor()
        self.assertEqual(len(extract.LATENCY_COLUMNS), 54)
        self.assertEqual(extract.LATENCY_COLUMNS[0], "idle_prefetch_p50_ms")
        self.assertEqual(extract.LATENCY_COLUMNS[-1], "uscan_storm_edit_max_ms")
        self.assertIn("scan_storm_thumb_p95_ms", extract.LATENCY_COLUMNS)
        self.assertEqual(
            extract.STORM_FAILED_COLUMNS,
            (
                "idle_storm_read_failed",
                "idle_storm_thumb_failed",
                "idle_storm_edit_failed",
                "scan_storm_read_failed",
                "scan_storm_thumb_failed",
                "scan_storm_edit_failed",
            ),
        )

    def test_percentile_matches_the_workload_interpolation(self):
        extract = extractor()
        self.assertEqual(extract.percentile([5.0], 95), 5.0)
        self.assertEqual(extract.percentile([1.0, 3.0], 95), 2.9)
        self.assertEqual(extract.percentile([0.0, 10.0, 20.0], 50), 10.0)


class MatrixStormTest(FixtureCase):
    """Storm bursts land in their background and kind, failures included."""

    def test_recorded_storm_runs_land_in_the_scan_background(self):
        cell = self.cell("storm-diff")
        matrix = extractor()._cell(extractor().parse_log(cell, profile="storm-diff"), {})

        # Two recorded runs: medians 6/5 ms -> 5.5, p95 worst 13 ms, max
        # worst 27 ms, and 68 failed ops in the first run plus none in the
        # second.
        self.assertEqual(matrix["scan_storm_edit_p50_ms"], 5.5)
        self.assertEqual(matrix["scan_storm_edit_p95_ms"], 13.0)
        self.assertEqual(matrix["scan_storm_edit_max_ms"], 27.0)
        self.assertEqual(matrix["scan_storm_edit_failed"], 68)
        # This cell ran no other storm kind.
        for column in (
            "scan_storm_read_p50_ms",
            "scan_storm_thumb_p50_ms",
            "idle_storm_edit_p50_ms",
            "idle_storm_edit_failed",
            "scan_storm_read_failed",
        ):
            self.assertEqual(matrix[column], "n/a", column)

    def test_storm_idle_runs_are_the_idle_background(self):
        # Values from the recorded storm-idle summary lines; the profile is
        # what separates the tree-at-rest burst from a concurrent one.
        row = {
            "storms": [
                {
                    "profile": "storm-idle",
                    "kind": "read",
                    "done": 80,
                    "total": 80,
                    "median": 0.004,
                    "p95": 0.015,
                    "max": 0.019,
                    "failed": 0,
                    "wall": 0.1,
                },
                {
                    "profile": "storm-idle",
                    "kind": "read",
                    "done": 80,
                    "total": 80,
                    "median": 0.004,
                    "p95": 0.016,
                    "max": 0.018,
                    "failed": 2,
                    "wall": 0.1,
                },
            ]
        }
        extract = extractor()
        self.assertEqual(extract._matrix_latency(row, "idle", "storm_read", "p50"), 4.0)
        self.assertEqual(extract._matrix_latency(row, "idle", "storm_read", "max"), 19.0)
        self.assertEqual(extract._matrix_failed(row, "idle", "read"), 2)
        self.assertEqual(extract._matrix_latency(row, "scan", "storm_read", "p50"), "n/a")


class MatrixProbeBucketTest(FixtureCase):
    """Upload probes are `uscan`, every other profile's are `scan`."""

    def test_upload_profile_puts_the_probes_in_uscan(self):
        cell = self.cell("control")  # recorded gate-upload-rep2 transcript
        matrix = extractor()._cell(extractor().parse_log(cell, profile="upload"), {})

        # Two recorded rounds: prefetch medians 9.874/7.929 ms -> 8.9;
        # fetch means 2.7/2.0 ms over 10 fetches each -> 2.35. The tails
        # are the extremes of those recorded rounds (the old worst-case
        # columns for this cell were 9.9 and 2.7).
        self.assertAlmostEqual(float(matrix["uscan_prefetch_p50_ms"]), 8.9, places=1)
        self.assertAlmostEqual(float(matrix["uscan_thumb_p50_ms"]), 2.35, places=1)
        self.assertEqual(matrix["uscan_prefetch_max_ms"], 9.9)
        self.assertEqual(matrix["uscan_thumb_max_ms"], 2.7)
        self.assertAlmostEqual(float(matrix["uscan_thumb_p95_ms"]), 2.7, places=1)
        for column in ("scan_prefetch_p50_ms", "scan_thumb_p50_ms", "scan_edit_p50_ms"):
            self.assertEqual(matrix[column], "n/a", column)

    def test_control_profile_puts_the_same_probes_in_scan(self):
        cell = self.cell("control")
        matrix = extractor()._cell(extractor().parse_log(cell, profile="control"), {})

        self.assertAlmostEqual(float(matrix["scan_prefetch_p50_ms"]), 8.9, places=1)
        self.assertAlmostEqual(float(matrix["scan_thumb_p50_ms"]), 2.35, places=1)
        # The per-round extremes are the values the report recorded as
        # worst-case before the matrix existed.
        self.assertEqual(matrix["scan_prefetch_max_ms"], 9.9)
        self.assertEqual(matrix["scan_thumb_max_ms"], 2.7)
        for column in ("uscan_prefetch_p50_ms", "uscan_thumb_p50_ms", "uscan_edit_p50_ms"):
            self.assertEqual(matrix[column], "n/a", column)

    def test_a_cell_without_probes_leaves_both_buckets_empty(self):
        cell = self.cell("storm-diff")
        matrix = extractor()._cell(extractor().parse_log(cell, profile="control"), {})
        for background in ("scan", "uscan"):
            for access in ("prefetch", "thumb", "edit"):
                column = f"{background}_{access}_p50_ms"
                self.assertEqual(matrix[column], "n/a", column)


class MatrixIdleTailTest(FixtureCase):
    """Idle p50 keeps the recorded median; p95/max pool the timed runs."""

    def _with_export(self, results) -> Path:
        cell = self.cell("control")
        (cell / "hyperfine.json").write_text(json.dumps({"results": results}))
        return cell

    def test_idle_tails_pool_every_post_scan_run(self):
        # Synthetic export: the post-scan command timings the cell writes.
        cell = self._with_export(
            [
                {"name": "compressed-get", "times": [0.007, 0.008, 0.050]},
                {"name": "prefetch", "times": [0.006, 0.007, 0.009]},
                {"name": "edit-rating", "times": [0.007, 0.0072, 0.030]},
            ]
        )
        matrix = extractor()._cell(extractor().parse_log(cell, profile="control"), {})

        # p50 is the transcript's post-scan median, unchanged by the export.
        self.assertEqual(matrix["idle_prefetch_p50_ms"], 7.1)
        self.assertEqual(matrix["idle_prefetch_p95_ms"], 8.8)
        self.assertEqual(matrix["idle_prefetch_max_ms"], 9.0)
        self.assertAlmostEqual(float(matrix["idle_thumb_p95_ms"]), 45.8, places=1)
        self.assertEqual(matrix["idle_thumb_max_ms"], 50.0)
        self.assertEqual(matrix["idle_edit_max_ms"], 30.0)

    def test_hyperfine_v2_measurements_are_read(self):
        cell = self._with_export(
            [
                {
                    "name": "prefetch",
                    "measurements": [
                        {"time_wall_clock": {"value": value}} for value in (0.006, 0.007, 0.011)
                    ],
                }
            ]
        )
        matrix = extractor()._cell(extractor().parse_log(cell, profile="control"), {})
        self.assertEqual(matrix["idle_prefetch_max_ms"], 11.0)

    def test_a_cell_without_the_export_falls_back_to_the_recorded_median(self):
        cell = self.cell("control")
        matrix = extractor()._cell(extractor().parse_log(cell, profile="control"), {})
        self.assertEqual(matrix["idle_prefetch_p50_ms"], 7.1)
        self.assertEqual(matrix["idle_prefetch_p95_ms"], 7.1)
        self.assertEqual(matrix["idle_prefetch_max_ms"], 7.1)


if __name__ == "__main__":
    unittest.main()
