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
                self.assertIn("falling back to per-round artifacts", stderr.getvalue())

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
        self.assertIn("falling back to per-round artifacts", stderr.getvalue())

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
                self.assertIn("falling back to per-round artifacts", stderr.getvalue())


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


class MatrixIdleTailTest(FixtureCase):
    """Idle p50 keeps the recorded median; p95/max pool the timed runs."""

    def _with_export(self, results) -> Path:
        cell = self.cell("control")
        (cell / "hyperfine.json").write_text(json.dumps({"results": results}))
        return cell

    def test_idle_tails_pool_every_post_scan_run(self):
        # Synthetic export: the post-scan command timings the cell writes.
        # p50 is the export's reported median; tails pool the timed runs.
        cell = self._with_export(
            [
                {"name": "compressed-get", "median": 0.0076, "times": [0.007, 0.008, 0.050]},
                {"name": "prefetch", "median": 0.0071, "times": [0.006, 0.007, 0.009]},
                {"name": "edit-rating", "median": 0.0072, "times": [0.007, 0.0072, 0.030]},
            ]
        )
        matrix = extractor()._cell(extractor().parse_log(cell, profile="control"), {})

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

    def test_a_cell_without_the_export_leaves_idle_cells_absent(self):
        cell = self.cell("control")
        (cell / "hyperfine.json").unlink()
        matrix = extractor()._cell(extractor().parse_log(cell, profile="control"), {})
        self.assertEqual(matrix["idle_prefetch_p50_ms"], "n/a")
        self.assertEqual(matrix["idle_prefetch_p95_ms"], "n/a")
        self.assertEqual(matrix["idle_prefetch_max_ms"], "n/a")


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

    def test_slim_cell_matches_with_and_without_events(self):
        with_events = extractor().parse_log(self.cell)
        (self.cell / "events.jsonl").unlink()
        without_events = extractor().parse_log(self.cell)
        self.assertEqual(with_events, without_events)


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

    def test_cell_carries_counters_and_process_metrics(self):
        row = {
            "benchmark": "cell",
            "observer_counters": [
                {"snapshot_memory_hit": 5, "snapshot_disk_fallback": 2},
                {"snapshot_memory_hit": 7, "snapshot_disk_fallback": 0},
            ],
            "vm_hwm_mb": 120.5,
            "temp_db_mb": 3.2,
        }

        cell = extractor()._cell(row, {})

        self.assertEqual(cell["snapshot_memory_hit"], "5,7")
        self.assertEqual(cell["snapshot_disk_fallback"], "2,0")
        self.assertEqual(cell["vm_hwm_mb"], 120.5)
        self.assertEqual(cell["temp_db_mb"], 3.2)

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
