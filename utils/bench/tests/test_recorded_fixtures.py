#!/usr/bin/env python3
"""Golden checks for the recorded parser fixtures.

Every file under `fixtures/` that a parser reads is a recorded artifact,
not a hand-written line. These tests pin the parser's output for those
files: replacing a fixture means updating the expectations here to the
new recording, so a fixture can never drift into a dialect the real
harness does not write.

See `fixtures/README.md` for the rule and each file's provenance.
"""

import importlib.util
import json
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
FIXTURES = Path(__file__).resolve().parent / "fixtures"
DUAL = FIXTURES / "events-dual"

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
    return load("extract_bench_golden", "extract-bench.py")


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

    def test_cell_with_the_recorded_server_log_extracts_every_stage_row(self):
        cell = self._cell()

        row = extractor().parse_log(cell)

        self.assertEqual(len(row["stages"]), 24)
        self.assertEqual([table["scan"] for table in row["drain_tables"]], [1, 2])
        build = [stage for stage in row["stages"] if stage["stage"] == "tree_refresh_build"]
        self.assertEqual([stage["p50_us"] for stage in build], [542, 1002])

    def test_dual_control_cell_stages_are_not_empty(self):
        # The S2 symptom was an empty stages.json for a real cell. The
        # recorded control cell must extract both of its tables.
        row = extractor().parse_log(DUAL / "control")

        self.assertEqual(len(row["drain_tables"]), 2)
        self.assertEqual(len(row["stages"]), 24)

    def _cell(self) -> Path:
        directory = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        shutil.copy(FIXTURES / "server-drain.log", directory / "server.log")
        (directory / "harness-a.log").write_text("scan wall median_ms=1000 over 1 run(s): 1000\n")
        (directory / "status-final.json").write_text(
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
        return directory


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

    def test_cell_with_the_recorded_stats_off_log_has_empty_stages(self):
        directory = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        shutil.copy(FIXTURES / "server-stats-off.log", directory / "server.log")
        (directory / "harness-a.log").write_text("scan wall median_ms=1000 over 1 run(s): 1000\n")
        (directory / "status-final.json").write_text(
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

        row = extractor().parse_log(directory)

        self.assertEqual(row["stages"], [])
        self.assertEqual(row["drain_tables"], [])


class BatteryStageSourceTest(unittest.TestCase):
    """The battery row's stage columns come from a recorded table."""

    def test_written_cell_stages_match_the_recorded_table(self):
        cell = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, cell, ignore_errors=True)
        outdir = cell / "gate-control"
        _write_cell(outdir)

        # The drain table lives in server.log, as the backend writes it;
        # the transcript never carries one. This is the S2 shape.
        server_log = (outdir / "server.log").read_text()
        self.assertEqual(
            extractor().parse_drain_tables(server_log),
            extractor().parse_drain_tables(FIXTURES / "server-drain.log"),
        )
        self.assertNotIn("observe drain:", (outdir / "harness-a.log").read_text())

        row = battery().build_row("gate", "control", outdir)
        self.assertEqual(row["tree_refresh_build_p50_us"], "542,1002")
        self.assertEqual(row["tree_refresh_build_max_us"], "1997,6069")
        self.assertEqual(row["filter_snapshot_clone_p50_us"], "419,1493")
        self.assertEqual(row["filter_snapshot_clone_max_us"], "1778,5236")


class InProcessRowStagesTest(unittest.TestCase):
    """A row built in-process carries the stages a standalone extract writes.

    The battery never runs `extract-bench.py`, so `battery extract` cannot
    depend on the `stages.json` only that script writes: the row sources
    its stages from the same `server.log` parse, without writing anything
    into the cell.
    """

    def test_in_process_stages_match_the_standalone_extract_output(self):
        cell = Path(tempfile.mkdtemp()) / "gate-control-rep1"
        self.addCleanup(shutil.rmtree, cell.parent, ignore_errors=True)
        _write_cell(cell)  # copies the recorded `server-drain.log` into it
        self.assertFalse((cell / "stages.json").exists())

        row = battery().build_row("gate", "control", cell)

        self.assertTrue(row["stages"])
        # Building a row is read-only w.r.t. the cell's artifacts.
        self.assertFalse((cell / "stages.json").exists())

        extracted = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, extracted, ignore_errors=True)
        copy = extracted / cell.name
        shutil.copytree(cell, copy)
        run = subprocess.run(
            [sys.executable, str(BENCH / "extract-bench.py"), str(copy)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        expected = json.loads((copy / "stages.json").read_text())
        self.assertTrue(expected)
        self.assertEqual(row["stages"], expected)


class DerivedEventStreamTest(unittest.TestCase):
    """The dual fixture's stream replays its recorded cell, not a guess.

    No recorded cell contains an `events.jsonl` (the stream postdates
    them), so `events.jsonl` is a replay of the cell's own artifacts. These
    checks re-derive it from the artifacts kept beside it; a fixture edit
    that invents a value fails here.
    """

    def test_storm_samples_replay_the_recorded_round_tsvs(self):
        cell = DUAL / "storm-diff"
        events = extractor().read_events(cell)

        replayed = []
        for run in (1, 2):
            path = cell / f"rounds-run-{run}" / f"storm-round-{run}.tsv"
            for line in path.read_text().splitlines():
                op, client, latency, code = line.split("\t")
                replayed.append(
                    (int(client), int(op), round(float(latency) * 1000.0, 6), int(code))
                )

        self.assertEqual(
            [(e["client"], e["op"], e["latency_ms"], e["code"]) for e in events.storm_samples],
            replayed,
        )

        text = (cell / "harness-a.log").read_text()
        verifies = re.findall(r"storm verify: matched (\d+)/(\d+) canary writes", text)
        self.assertEqual(
            [(e["matched"], e["total"]) for e in events.verify_results],
            [(int(matched), int(total)) for matched, total in verifies],
        )

    def test_control_uploads_and_scans_replay_the_recorded_transcript(self):
        cell = DUAL / "control"
        events = extractor().read_events(cell)
        text = (cell / "harness-a.log").read_text()

        uploads = re.findall(r"upload file=(\S+) submit_ms=(\d+) visible_ms=(\d+)", text)
        self.assertEqual(
            [(e["file"], e["submit_ms"], e["visible_ms"]) for e in events.upload_files],
            [(name, int(submit), int(visible)) for name, submit, visible in uploads],
        )
        scans = re.findall(r"scan run (\d+)/\d+: state=(\w+) polls=\d+ wall_ms=(\d+)", text)
        self.assertEqual(
            [(e["run"], e["state"], e["wall_ms"]) for e in events.scan_states],
            [(int(run), state, int(wall)) for run, state, wall in scans],
        )
        fetches = re.findall(
            r"probe round=\d+ rows=(\d+) state=\w+ fetch: thumb: n=(\d+) mean=([\d.]+)", text
        )
        self.assertEqual(
            [
                (e["rows"], e["fetch_n"], round(e["fetch_mean"], 4))
                for e in events.probe_rounds
                if "fetch_n" in e
            ],
            [(int(rows), int(n), float(mean)) for rows, n, mean in fetches],
        )
        # Full precision comes from the per-round probe TSVs, not the
        # transcript's 4-decimal mean.
        means = []
        for run in (1, 2):
            path = cell / f"rounds-run-{run}" / "probe-round-1.tsv"
            latencies = [float(line.split("\t")[2]) for line in path.read_text().splitlines()]
            means.append((sum(latencies) / len(latencies), min(latencies), max(latencies)))
        self.assertEqual(
            [
                (e["fetch_mean"], e["fetch_min"], e["fetch_max"])
                for e in events.probe_rounds
                if "fetch_n" in e
            ],
            means,
        )


def _write_cell(outdir: Path) -> None:
    """The battery tests' cell builder, loaded once from `test_battery`."""
    module = sys.modules.get("test_battery") or load("test_battery", "tests/test_battery.py")
    module.write_cell(outdir)


if __name__ == "__main__":
    unittest.main()
