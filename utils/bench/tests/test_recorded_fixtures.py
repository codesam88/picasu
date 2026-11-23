#!/usr/bin/env python3
"""Golden checks for the recorded parser fixtures.

Every file under `fixtures/` that a parser reads is a recorded artifact,
not a hand-written line. These tests pin the parser's output for those
files: replacing a fixture means updating the expectations here to the
new recording, so a fixture can never drift into a dialect the real
harness does not write.

See `fixtures/README.md` for the rule and each file's provenance. Fixture
directory names are the labels the cells were recorded under and stay as
they are; the current profile set lives in `scenarios.py`.
"""

import shutil
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))
FIXTURES = Path(__file__).resolve().parent / "fixtures"
CELL = FIXTURES / "recorded" / "probe-50"

_LOADED: dict = {}


def load(name: str, filename: str):
    if name not in _LOADED:
        if "extract_bench" in filename:
            import extract_bench as module
        elif "battery" in filename:
            import battery as module
        else:
            module = __import__(name)
        sys.modules[name] = module
        _LOADED[name] = module
    return _LOADED[name]


def extractor():
    return load("extract_bench_golden", "extract_bench.py")


def battery():
    return load("battery_golden", "battery.py")


def stage_row(**fields) -> dict:
    return {
        "stage": fields["stage"],
        "n": fields["n"],
        "p50_us": fields["p50_us"],
        "p95_us": fields["p95_us"],
        "max_us": fields["max_us"],
    }


class RecordedDrainTest(unittest.TestCase):
    """`server-drain.log`: the stats-on record with two scan tables."""

    def test_both_recorded_scan_tables_parse_verbatim(self):
        tables = extractor().parse_drain_tables(FIXTURES / "server-drain.log")

        self.assertEqual([table["scan"] for table in tables], [1, 2])
        self.assertEqual(
            [table["context"] for table in tables],
            [
                "scan job=1 state=Completed scanned=100 matched=100 processed=15 failed=0",
                "scan job=2 state=Completed scanned=101 matched=100 processed=100 failed=0",
            ],
        )
        self.assertEqual(
            tables[0]["stages"],
            [
                stage_row(
                    stage="filter_snapshot_clone", n=31, p50_us=419, p95_us=1731, max_us=1778
                ),
                stage_row(stage="filter_par_iter", n=31, p50_us=2397, p95_us=12216, max_us=16993),
                stage_row(stage="compute_locate", n=31, p50_us=0, p95_us=0, max_us=0),
                stage_row(stage="tree_refresh_total", n=81, p50_us=690, p95_us=1354, max_us=2178),
                stage_row(stage="tree_refresh_build", n=81, p50_us=542, p95_us=1092, max_us=1997),
                stage_row(stage="tree_refresh_sort", n=81, p50_us=14, p95_us=32, max_us=39),
                stage_row(stage="tree_refresh_write_wait", n=81, p50_us=0, p95_us=0, max_us=0),
                stage_row(
                    stage="cpu_permit_wait", n=256, p50_us=6887, p95_us=302213, max_us=420874
                ),
                stage_row(stage="cpu_exec", n=256, p50_us=264, p95_us=964988, max_us=1342541),
                stage_row(stage="prefetch_total", n=71, p50_us=29, p95_us=10350, max_us=18300),
                stage_row(stage="blocking_queue_wait", n=71, p50_us=28, p95_us=866, max_us=1827),
                stage_row(stage="interactive_exec", n=0, p50_us=0, p95_us=0, max_us=0),
            ],
        )
        second = {row["stage"]: row for row in tables[1]["stages"]}
        self.assertEqual(
            second["tree_refresh_build"],
            stage_row(stage="tree_refresh_build", n=60, p50_us=1002, p95_us=1101, max_us=6069),
        )
        self.assertEqual(
            second["filter_snapshot_clone"],
            stage_row(stage="filter_snapshot_clone", n=3, p50_us=1493, p95_us=5236, max_us=5236),
        )
        self.assertEqual(
            second["interactive_exec"],
            stage_row(stage="interactive_exec", n=0, p50_us=0, p95_us=0, max_us=0),
        )

    def test_recorded_probe_cell_stages_are_not_empty(self):
        # The S2 symptom was an empty stages.json for a real cell. The
        # recorded probe cell must extract both of its tables.
        row = extractor().parse_log(CELL)

        self.assertEqual(len(row["drain_tables"]), 2)
        self.assertEqual(len(row["stages"]), 24)


class RecordedStatsOffTest(unittest.TestCase):
    """`server-stats-off.log`: the same run shape with observer tables off."""

    def test_stats_off_record_carries_no_tables_and_no_stats_banner(self):
        text = (FIXTURES / "server-stats-off.log").read_text()

        # A stats-off server never announces per-scan tables, and writes
        # no `observe drain:` block. The fabricated fixture this replaced
        # kept the stats-on banner while deleting the tables, which no
        # real cell can produce.
        self.assertNotIn("observer stats enabled", text)
        self.assertNotIn("observe drain:", text)
        self.assertEqual(extractor().parse_drain_tables(FIXTURES / "server-stats-off.log"), [])

    def test_cell_without_an_event_stream_is_rejected(self):
        directory = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        shutil.copy(FIXTURES / "server-stats-off.log", directory / "server.log")
        (directory / "harness-a.log").write_text("scan wall median_ms=1000 over 1 run(s): 1000\n")

        # The event stream is the measurement record; there is no artifact
        # fallback to silently pool instead.
        with self.assertRaises(extractor().EventsError):
            extractor().parse_log(directory)

    def test_stats_off_log_parses_to_no_tables(self):
        self.assertEqual(extractor().parse_drain_tables(FIXTURES / "server-stats-off.log"), [])


class RecordedCellGoldenTest(unittest.TestCase):
    """The recorded cell parses to the values its artifacts record."""

    def test_recorded_probe_cell_parses_to_its_recorded_values(self):
        row = extractor().parse_log(CELL)

        self.assertEqual(row["scan_wall_s"], 11.23)
        self.assertEqual(row["scan_runs_ms"], [11.14, 11.32])
        self.assertEqual(row["probe_rounds"], 20)
        self.assertEqual(row["probe_rounds_total"], 20)
        self.assertEqual(row["probe_rounds_valid"], 20)
        self.assertEqual(row["phase_invalid"], 0)
        self.assertEqual(row["warmup_valid"], "true")
        self.assertEqual(row["scan_thumb_n"], 56)
        self.assertEqual(len(row["drain_tables"]), 2)
        self.assertEqual(len(row["stages"]), 24)
        samples = row["samples"]
        self.assertEqual(len(samples["latency|scan|prefetch"]), 60)
        self.assertEqual(len(samples["latency|idle|prefetch"]), 3)
        self.assertEqual(len(samples["latency|scan|scroll_bar"]), 20)
        self.assertEqual(len(samples["latency|scan|scroll_rows"]), 142)
        self.assertEqual(len(samples["latency|scan|thumb"]), 56)
        self.assertEqual(len(samples["stage|scan|cpu_exec"]), 2)


def _write_cell(outdir: Path) -> None:
    """The battery tests' cell builder, loaded once from `test_battery`."""
    tests_dir = BENCH / "tests"
    if str(tests_dir) not in sys.path:
        sys.path.insert(0, str(tests_dir))
    import test_battery

    test_battery.write_cell(outdir)


if __name__ == "__main__":
    unittest.main()
