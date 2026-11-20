"""Unit tests for environment snapshot capture (no host access needed)."""

import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))

import environment  # noqa: E402


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


class CollectTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.sys = self.root / "sys"
        self.proc = self.root / "proc"

    def test_collects_from_fake_sysfs_and_proc(self):
        write(self.sys / "devices/system/cpu/cpufreq/policy0/scaling_cur_freq", "2400000\n")
        write(self.sys / "devices/system/cpu/cpufreq/policy0/scaling_max_freq", "2400000\n")
        write(self.sys / "devices/system/cpu/cpufreq/policy0/scaling_governor", "powersave\n")
        write(self.sys / "devices/system/cpu/cpufreq/policy0/scaling_driver", "intel_pstate\n")
        write(self.sys / "devices/system/cpu/intel_pstate/no_turbo", "0\n")
        write(self.proc / "cpuinfo", "model name\t: Fake CPU 9000\n")
        write(self.proc / "meminfo", "MemTotal:        65804280 kB\n")
        record = environment.collect_environment(
            sys_root=self.sys, proc_root=self.proc, git_root=self.root
        )
        self.assertEqual(record["freq_cur_mhz"], 2400)
        self.assertEqual(record["freq_max_mhz"], 2400)
        self.assertEqual(record["governor"], "powersave")
        self.assertEqual(record["cpu_model"], "Fake CPU 9000")
        self.assertEqual(record["mem_total_kb"], 65804280)
        self.assertEqual(record["turbo_disabled"], 0)
        self.assertIn("started_utc", record)
        self.assertIsNone(record["ended_utc"])

    def test_missing_files_become_unknown_not_errors(self):
        record = environment.collect_environment(sys_root=self.sys, proc_root=self.proc)
        self.assertEqual(record["freq_cur_mhz"], environment.UNKNOWN)
        self.assertEqual(record["governor"], environment.UNKNOWN)
        self.assertEqual(record["cpu_model"], environment.UNKNOWN)
        self.assertEqual(record["mem_total_kb"], environment.UNKNOWN)
        self.assertEqual(record["turbo_disabled"], environment.UNKNOWN)

    def test_level_is_recorded_when_given(self):
        record = environment.collect_environment(
            sys_root=self.sys, proc_root=self.proc, level="gate"
        )
        self.assertEqual(record["level"], "gate")

    def test_finish_stamps_end_without_touching_start(self):
        record = environment.collect_environment(sys_root=self.sys, proc_root=self.proc)
        started = record["started_utc"]
        done = environment.finish_environment(dict(record))
        self.assertEqual(done["started_utc"], started)
        self.assertIsNotNone(done["ended_utc"])
        self.assertIn("end_load_1", done)

    def test_roundtrip_through_disk(self):
        record = environment.collect_environment(sys_root=self.sys, proc_root=self.proc)
        path = self.root / "out" / environment.ENVIRONMENT_FILENAME
        environment.write_environment(path, record)
        loaded = environment.load_environment(path)
        self.assertEqual(loaded, record)

    def test_load_missing_or_broken_returns_none(self):
        self.assertIsNone(environment.load_environment(self.root / "absent.json"))
        bad = self.root / "bad.json"
        bad.write_text("{nope")
        self.assertIsNone(environment.load_environment(bad))


if __name__ == "__main__":
    unittest.main()
