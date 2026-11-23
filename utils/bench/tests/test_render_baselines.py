#!/usr/bin/env python3
"""Tests for render_baselines.py: per-image tables and baseline-only rendering."""

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))

import battery  # noqa: E402
import render_baselines as rb  # noqa: E402
import scenarios  # noqa: E402

CELL = f"{scenarios.PROBE}-100"


def baseline_row(cell=CELL, **overrides):
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
                "probe_rounds_total": 20,
                "probe_rounds_valid": 20,
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

    def test_measured_profiles_and_sections_are_rendered(self):
        probe_cell = CELL
        upload_cell = f"{scenarios.IDLE_UPLOAD}-100"
        rows = [
            baseline_row(probe_cell),
            baseline_row(
                probe_cell, family="wall", background="scan", subject="scan", value="11.0"
            ),
            baseline_row(
                upload_cell, family="upload", background="idle", subject="submit", value="12.0"
            ),
            baseline_row(
                probe_cell, family="gauge", background="idle", subject="vm_hwm_mb", value="300.0"
            ),
        ]
        metadata = {}
        metadata.update(meta(probe_cell, scenarios.PROBE, 100))
        metadata.update(meta(upload_cell, scenarios.IDLE_UPLOAD, 100))
        text = rb.render(rows, metadata, [])
        for section in ("Measurement validity", "Foreground latency", "Scan time",
                        "Memory and storage footprint"):
            self.assertIn(f"## {section}", text)
        self.assertIn(scenarios.probe_label("prefetch"), text)
        self.assertIn(scenarios.upload_label("submit"), text)

    def test_validity_pairs_backgrounds_and_reports_pooled_accesses(self):
        storm_idle_cell = f"{scenarios.storm_profile('idle', 'read', '8')}-100"
        storm_scan_cell = f"{scenarios.storm_profile('scan', 'read', '8')}-100"
        upload_cell = f"{scenarios.SCAN_UPLOAD}-100"
        rows = [
            baseline_row(CELL, background="idle", subject="prefetch", n_samples="3"),
            baseline_row(CELL, background="scan", subject="prefetch", n_samples="1"),
            baseline_row(CELL, background="scan", subject="prefetch", stat="p95", n_samples="1"),
            baseline_row(
                storm_idle_cell, background="idle", subject="prefetch", n_samples="20"
            ),
            baseline_row(
                storm_scan_cell, background="scan", subject="prefetch", n_samples="21311"
            ),
            baseline_row(
                upload_cell,
                family="upload",
                background="scan",
                subject="submit",
                n_samples="20",
            ),
            baseline_row(
                upload_cell,
                family="upload",
                background="scan",
                subject="visible",
                n_samples="20",
            ),
        ]
        metadata = {}
        metadata.update(meta(CELL, scenarios.PROBE, 100))
        metadata.update(meta(storm_idle_cell, scenarios.storm_profile("idle", "read", "8"), 100))
        metadata.update(meta(storm_scan_cell, scenarios.storm_profile("scan", "read", "8"), 100))
        metadata.update(meta(upload_cell, scenarios.SCAN_UPLOAD, 100))
        text = rb.render(rows, metadata, [])
        validity = text.split("## Measurement validity", 1)[1].split("\n## ", 1)[0]
        header = next(line for line in validity.splitlines() if line.startswith("| profile"))
        for column in ("idle accesses", "scan accesses", "idle invalid", "scan failed"):
            self.assertIn(column, header)
        lines = {
            line.split("|")[1].strip(): [part.strip() for part in line.split("|")[2:-1]]
            for line in validity.splitlines()
            if line.startswith("|") and "profile" not in line
        }
        # p95 is not counted again; probe invalid belongs to the scan guard.
        self.assertEqual(
            lines[scenarios.PROBE],
            ["3", "n/a", "n/a", "1", "0", "0"],
        )
        # Storm cells pair the idle and scan recordings of the same load.
        self.assertEqual(
            lines[scenarios.storm_label("read", "8")],
            ["20", "0", "0", "21311", "0", "0"],
        )
        # `visible` observes the same upload again and is not counted twice.
        self.assertEqual(
            lines[scenarios.UPLOAD_LOOP_LABEL],
            ["n/a", "n/a", "n/a", "20", "0", "0"],
        )

    def test_no_load_row_is_the_median_of_the_idle_storm_scans(self):
        def wall_row(cell, value):
            return baseline_row(
                cell, family="wall", background="scan", subject="scan", value=value
            )

        idle_cells = {
            f"{scenarios.storm_profile('idle', kind, level)}-100": (
                scenarios.storm_profile("idle", kind, level),
                value,
            )
            for kind, level, value in (
                ("read", "8", "15.0"),
                ("read", "16n", "12.0"),
                ("edit", "8", "13.0"),
            )
        }
        storm_cell = f"{scenarios.storm_profile('scan', 'read', '8')}-100"
        upload_cell = f"{scenarios.SCAN_UPLOAD}-100"
        rows = [wall_row(cell, value) for cell, (_profile, value) in idle_cells.items()]
        rows += [
            wall_row(storm_cell, "11.0"),
            wall_row(upload_cell, "15.0"),
        ]
        metadata = {}
        for cell, (profile, _value) in idle_cells.items():
            metadata.update(meta(cell, profile, 100))
        metadata.update(meta(storm_cell, scenarios.storm_profile("scan", "read", "8"), 100))
        metadata.update(meta(upload_cell, scenarios.SCAN_UPLOAD, 100))
        text = rb.render(rows, metadata, [])
        scan_time = text.split("## Scan time", 1)[1].split("\n## ", 1)[0]
        lines = {
            line.split("|")[1].strip(): line
            for line in scan_time.splitlines()
            if line.startswith("|") and "load profile" not in line
        }
        # The median of 15.0, 12.0 and 13.0, not the read-8 cell alone.
        self.assertIn("13.0 s (7.7 img/s)", lines[scenarios.NO_LOAD_LABEL])
        self.assertIn("11.0 s (9.1 img/s)", lines[scenarios.storm_label("read", "8")])

    def test_suspect_cells_warn_but_stay_rendered(self):
        idle_cell = f"{scenarios.storm_profile('idle', 'read', '8')}-100"
        scan_cell = f"{scenarios.storm_profile('scan', 'read', '8')}-100"
        probe_cell = f"{scenarios.PROBE}-100"
        rows = [
            baseline_row(idle_cell, background="idle", subject="prefetch"),
            baseline_row(scan_cell, background="scan", subject="prefetch"),
            baseline_row(probe_cell, subject="prefetch"),
        ]
        metadata = {}
        metadata.update(meta(idle_cell, scenarios.storm_profile("idle", "read", "8"), 100))
        metadata.update(meta(scan_cell, scenarios.storm_profile("scan", "read", "8"), 100))
        metadata.update(meta(probe_cell, scenarios.PROBE, 100))
        # 1 of 20 rounds is within the 10% budget: no warning.
        metadata[idle_cell]["validity"]["phase_invalid"] = 1
        metadata[idle_cell]["validity"]["probe_rounds_total"] = 20
        # 3 of 20 is past it: the cell stays rendered, and the renderer
        # warns on stderr.
        metadata[scan_cell]["validity"]["phase_invalid"] = 3
        metadata[scan_cell]["validity"]["probe_rounds_total"] = 20
        err, text = io.StringIO(), io.StringIO()
        with contextlib.redirect_stderr(err), contextlib.redirect_stdout(text):
            rendered = rb.render(rows, metadata, [])
        self.assertIn(scenarios.storm_label("read", "8"), rendered)
        self.assertIn("WARNING: cell ", err.getvalue())
        self.assertIn(f"{scan_cell}: 3/20 guard rejections", err.getvalue())

        # A rejection without a denominator warns as well.
        metadata[probe_cell]["validity"]["phase_invalid"] = 1
        metadata[probe_cell]["validity"]["probe_rounds_total"] = "n/a"
        err, text = io.StringIO(), io.StringIO()
        with contextlib.redirect_stderr(err), contextlib.redirect_stdout(text):
            rb.render(rows, metadata, [])
        self.assertIn(f"{probe_cell}: 1 guard rejection(s) without", err.getvalue())

    def test_tables_are_ordered_by_image_count(self):
        counts = (50, 100, 200)
        rows = [baseline_row(f"{scenarios.PROBE}-{count}") for count in counts]
        metadata = {}
        for count in counts:
            metadata.update(meta(f"{scenarios.PROBE}-{count}", scenarios.PROBE, count))
        text = rb.render(rows, metadata, [])
        positions = [text.index(f"### {count} images") for count in counts]
        self.assertEqual(positions, sorted(positions))

    def test_cell_name_and_image_count_leave_the_rows(self):
        rows = [baseline_row(), baseline_row(background="scan", value="9.0")]
        cell = CELL
        text = rb.render(rows, meta(cell, scenarios.PROBE, 100), [])
        self.assertNotIn(cell, text)
        self.assertNotIn("| images", text)
        self.assertIn(scenarios.PROBE, text)
        self.assertIn("| idle", text)
        self.assertIn("| scan", text)

    def test_interval_is_the_half_width_of_the_recorded_bounds(self):
        rows = [
            baseline_row(ci_low="6.5", ci_high="7.1"),
            baseline_row(subject="thumb", ci_low="7.0", ci_high="7.0"),
        ]
        text = rb.render(rows, meta(CELL, scenarios.PROBE, 100), [])
        for row in rows:
            half_width = (float(row["ci_high"]) - float(row["ci_low"])) / 2
            self.assertIn(f"{row['value']} ±{half_width:.1f}", text)

    def test_absent_interval_is_marked(self):
        rows = [baseline_row(ci_low="n/a", ci_high="n/a")]
        text = rb.render(rows, meta(CELL, scenarios.PROBE, 100), [])
        self.assertIn("±n/a", text)

    def test_sample_count_is_not_rendered(self):
        text = rb.render([baseline_row()], meta(CELL, scenarios.PROBE, 100), [])
        self.assertNotIn("n_samples", text)
        self.assertNotIn("|   n |", text)

    def test_memory_table_rows_in_fixed_profile_order(self):
        probe_cell = CELL
        storm_profile = scenarios.storm_profile("idle", "read", "8")
        storm_cell = f"{storm_profile}-100"
        rows = [
            baseline_row(
                probe_cell, family="gauge", background="idle", subject="vm_hwm_mb", value="300.0"
            ),
            baseline_row(
                probe_cell, family="gauge", background="idle", subject="temp_db_mb", value="1.0"
            ),
            baseline_row(
                storm_cell, family="gauge", background="idle", subject="vm_hwm_mb", value="900.0"
            ),
            baseline_row(
                storm_cell, family="gauge", background="idle", subject="temp_db_mb", value="1.0"
            ),
        ]
        metadata = {}
        metadata.update(meta(probe_cell, scenarios.PROBE, 100))
        metadata.update(meta(storm_cell, storm_profile, 100))
        text = rb.render(rows, metadata, [])
        memory = text.split("## Memory and storage footprint", 1)[1].split("\n## ", 1)[0]
        # Alphabetical profile order, independent of the values: a diff
        # between recordings moves values, not rows.
        self.assertLess(memory.index("idle-storm"), memory.index("| probe "))

    def test_baselines_file_is_the_only_input(self):
        rows = [baseline_row()]
        metadata = meta(CELL, scenarios.PROBE, 100)
        path = self.root / "baselines.tsv"
        path.write_text(battery.baselines_text(rows, [], metadata))
        out = self.root / "out.md"
        rc = rb.main(["--baselines", str(path), "--out", str(out)])
        self.assertEqual(rc, 0)
        self.assertEqual(out.read_text(), rb.render(rows, metadata, []))

    def test_terms_follow_the_environment(self):
        text = rb.render([baseline_row()], meta(CELL, scenarios.PROBE, 100), [])
        self.assertLess(text.index("## Run environment"), text.index("## Terms"))
        self.assertLess(text.index("## Terms"), text.index("## Measurement validity"))

    def test_environment_is_printed_once_from_the_earliest_run(self):
        runs = [
            {"hostname": "h", "started_utc": "2026-01-02", "load_1": 5.0, "freq_cur_mhz": 1800},
            {"hostname": "h", "started_utc": "2026-01-01", "load_1": 1.0, "freq_cur_mhz": 2400},
        ]
        text = rb.render([baseline_row()], meta(CELL, scenarios.PROBE, 100), runs)
        self.assertEqual(text.count("**host**"), 1)
        self.assertIn("1.00", text)
        self.assertNotIn("5.00", text)
        self.assertIn("2400", text)

    def test_environment_mismatch_refuses_the_document(self):
        runs = [
            {"hostname": "h", "cpu_model": "a", "started_utc": "2026-01-01"},
            {"hostname": "h", "cpu_model": "b", "started_utc": "2026-01-02"},
        ]
        with self.assertRaises(ValueError):
            rb.render([baseline_row()], meta(CELL, scenarios.PROBE, 100), runs)

    def test_environment_ignores_frequency_and_load_differences(self):
        runs = [
            {"hostname": "h", "started_utc": "2026-01-01", "load_1": 1.0, "freq_cur_mhz": 1800},
            {"hostname": "h", "started_utc": "2026-01-02", "load_1": 9.0, "freq_cur_mhz": 2400},
        ]
        text = rb.render([baseline_row()], meta(CELL, scenarios.PROBE, 100), runs)
        self.assertIn("**host**: h", text)

    def test_environment_bullets(self):
        runs = [{"hostname": "h", "cpu_count": 8, "cpu_model": "m", "load_1": 1.0}]
        text = rb.render([baseline_row()], meta(CELL, scenarios.PROBE, 100), runs)
        self.assertIn("**host**: h", text)
        self.assertIn("1.00", text)

    def test_upload_rows_share_the_load_profile_row(self):
        rows = []
        metadata = {}
        for profile, background, value in (
            (scenarios.IDLE_UPLOAD, "idle", "12.0"),
            (scenarios.SCAN_UPLOAD, "scan", "18.0"),
        ):
            cell = f"{profile}-100"
            rows.append(
                baseline_row(
                    cell,
                    family="upload",
                    background=background,
                    subject="submit",
                    value=value,
                )
            )
            metadata.update(meta(cell, profile, 100))
        text = rb.render(rows, metadata, [])
        table = text.split("## Foreground latency", 1)[1].split("\n## ", 1)[0]
        self.assertIn(f"| {scenarios.upload_label('submit')} ", table)
        self.assertIn("12.0", table)
        self.assertIn("18.0", table)


if __name__ == "__main__":
    unittest.main()
