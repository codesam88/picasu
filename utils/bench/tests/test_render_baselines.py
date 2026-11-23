#!/usr/bin/env python3
"""Tests for render_baselines.py: per-image tables and baseline-only rendering."""

import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))

import battery  # noqa: E402
import render_baselines as rb  # noqa: E402


def baseline_row(cell="control-100", **overrides):
    row = {
        "cell": cell,
        "family": "latency",
        "background": "idle",
        "subject": "prefetch",
        "stat": "p50",
        "value": "6.8",
        "unit": "ms",
        "n_samples": "99",
        "n_clusters": "33",
        "ci_low": "6.5",
        "ci_high": "7.1",
        "recorded": "2026-01-01 main abc1234",
        "note": "recorded",
    }
    row.update(overrides)
    return row


def meta(cell, profile, images, **validity):
    return {
        cell: {
            "identity": {"cell": cell, "source": "battery", "profile": profile, "images": images},
            "validity": {
                "reps": 1,
                "probe_rounds_valid": 20,
                "probe_coverage_pct": 28.3,
                "storm_coverage_pct": "n/a",
                "warmup_valid": "true",
                "phase_invalid": 0,
                "failed": 0,
                **validity,
            },
        }
    }


class RenderBaselinesTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_sections_and_intros(self):
        rows = [baseline_row()]
        text = rb.render(rows, meta("control-100", "control", 100), [])
        for header in (
            "## Run environment",
            "## Measurement validity",
            "## Idle access latency",
            "## During-scan access latency",
            "## Concurrent load",
            "## Scan wall and stages",
            "## Upload",
            "## Memory and storage footprint",
            "## Failed operations",
            "## Terms",
        ):
            self.assertIn(header, text)
        for sentence in (
            "serving floor",
            "indexing-contention",
            "one row per configuration, sorted by peak memory",
            "operations that did not return 200",
        ):
            self.assertIn(sentence, text)

    def test_tables_are_grouped_by_image_count(self):
        rows = [
            baseline_row("control-100"),
            baseline_row("control-200"),
            baseline_row("control-50"),
        ]
        metadata = {}
        for cell, images in (("control-100", 100), ("control-200", 200), ("control-50", 50)):
            metadata.update(meta(cell, "control", images))
        text = rb.render(rows, metadata, [])
        self.assertIn("### 50 images", text)
        self.assertIn("### 100 images", text)
        self.assertIn("### 200 images", text)
        self.assertLess(text.index("### 50 images"), text.index("### 100 images"))

    def test_cell_name_and_image_count_leave_the_rows(self):
        rows = [baseline_row()]
        text = rb.render(rows, meta("control-100", "control", 100), [])
        idle = text.split("## Idle access latency", 1)[1].split("\n## ", 1)[0]
        self.assertIn("| profile", idle)
        self.assertIn("| control", idle)
        self.assertNotIn("control-100", idle)
        self.assertNotIn("| images", idle)

    def test_interval_is_a_half_width_and_always_shown(self):
        rows = [
            baseline_row(ci_low="6.5", ci_high="7.1"),
            baseline_row(subject="thumb", ci_low="7.0", ci_high="7.0"),
        ]
        text = rb.render(rows, meta("control-100", "control", 100), [])
        self.assertIn("6.8 ±0.3", text)
        self.assertIn("6.8 ±0.0", text)

    def test_absent_interval_is_marked(self):
        rows = [baseline_row(ci_low="n/a", ci_high="n/a")]
        text = rb.render(rows, meta("control-100", "control", 100), [])
        self.assertIn("±n/a", text)

    def test_sample_count_is_not_rendered(self):
        text = rb.render([baseline_row()], meta("control-100", "control", 100), [])
        self.assertNotIn("n_samples", text)
        self.assertNotIn("|   n |", text)

    def test_memory_table_sorted_by_peak(self):
        rows = [
            baseline_row(
                "control-100", family="gauge", background="idle", subject="vm_hwm_mb", value="300.0"
            ),
            baseline_row(
                "control-100", family="gauge", background="idle", subject="temp_db_mb", value="1.0"
            ),
            baseline_row(
                "storm-read-8-100",
                family="gauge",
                background="idle",
                subject="vm_hwm_mb",
                value="900.0",
            ),
            baseline_row(
                "storm-read-8-100",
                family="gauge",
                background="idle",
                subject="temp_db_mb",
                value="1.0",
            ),
        ]
        metadata = {}
        metadata.update(meta("control-100", "control", 100))
        metadata.update(meta("storm-read-8-100", "storm-read-8", 100))
        text = rb.render(rows, metadata, [])
        memory = text.split("## Memory and storage footprint", 1)[1].split("\n## ", 1)[0]
        self.assertLess(memory.index("storm-read-8"), memory.index("control"))
        self.assertIn("peak MiB", memory)

    def test_counts_are_integers(self):
        rows = [
            baseline_row(
                family="count",
                background="scan",
                subject="prefetch",
                stat="sum",
                value="3",
                unit="ops",
            )
        ]
        text = rb.render(rows, meta("control-100", "control", 100), [])
        failed = text.split("## Failed operations", 1)[1].split("\n## ", 1)[0]
        self.assertIn(" 3 ", failed)
        self.assertNotIn("3.0", failed)

    def test_baselines_file_is_the_only_input(self):
        rows = [baseline_row()]
        metadata = meta("control-100", "control", 100)
        path = self.root / "baselines.tsv"
        path.write_text(battery.baselines_text(rows, [], metadata))
        out = self.root / "out.md"
        rc = rb.main(["--baselines", str(path), "--out", str(out)])
        self.assertEqual(rc, 0)
        self.assertIn("6.8 ±0.3", out.read_text())

    def test_environment_bullets(self):
        runs = [{"hostname": "h", "cpu_count": 8, "cpu_model": "m", "load_1": 1.0}]
        text = rb.render([baseline_row()], meta("control-100", "control", 100), runs)
        self.assertIn("**host**: h", text)
        self.assertIn("1.00", text)


if __name__ == "__main__":
    unittest.main()
