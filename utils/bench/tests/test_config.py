#!/usr/bin/env python3
"""Tests for config.py: the shared validated configuration.

The per-layer suites pin every layer's accepted and rejected values; this
file pins the shared validators and the one-error-shape contract: the
launcher, the cell orchestrator and the workload reject the same bad
environment value through the same rule, with the same message.
"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))


def load_module(name: str, filename: str):
    """Import one bench module by name."""
    module = sys.modules.get(name)
    if module is not None:
        return module
    module = __import__(name)
    sys.modules[name] = module
    return module


cf = load_module("config", "config.py")
bl = load_module("backend_launcher", "backend_launcher.py")
oc = load_module("run_cell", "run_cell.py")
workload = load_module("workload", "workload.py")
pf = load_module("preflight", "preflight.py")


class ValidatorTest(unittest.TestCase):
    """The shared rules: positive int/float, port range, path rules."""

    def test_integer_accepts_default_and_override(self):
        self.assertEqual(cf.integer({}, "IMAGES", 100), 100)
        self.assertEqual(cf.integer({"IMAGES": "7"}, "IMAGES", 100), 7)

    def test_integer_rejects_non_numeric_and_below_minimum(self):
        with self.assertRaises(cf.ConfigError) as ctx:
            cf.integer({"IMAGES": "many"}, "IMAGES", 100, minimum=1)
        self.assertEqual(str(ctx.exception), "IMAGES must be an integer, got 'many'")
        with self.assertRaises(cf.ConfigError) as ctx:
            cf.integer({"IMAGES": "0"}, "IMAGES", 100, minimum=1)
        self.assertEqual(str(ctx.exception), "IMAGES must be >= 1, got 0")

    def test_number_rejects_non_numeric_and_reports_the_bound(self):
        with self.assertRaises(cf.ConfigError) as ctx:
            cf.number({"UPLOAD_TIMEOUT_S": "soon"}, "UPLOAD_TIMEOUT_S", 30.0, minimum=0.1)
        self.assertEqual(str(ctx.exception), "UPLOAD_TIMEOUT_S must be a number, got 'soon'")
        with self.assertRaises(cf.ConfigError) as ctx:
            cf.number({"UPLOAD_TIMEOUT_S": "0"}, "UPLOAD_TIMEOUT_S", 30.0, minimum=0.1)
        self.assertEqual(str(ctx.exception), "UPLOAD_TIMEOUT_S must be >= 0.1, got 0.0")

    def test_exclusive_float_bound(self):
        self.assertEqual(cf.number({"X": "0.5"}, "X", 1.0, exclusive=True), 0.5)
        with self.assertRaises(cf.ConfigError) as ctx:
            cf.number({"X": "0"}, "X", 1.0, exclusive=True)
        self.assertEqual(str(ctx.exception), "X must be > 0, got 0.0")

    def test_port_range(self):
        self.assertEqual(cf.validate_port("5800"), 5800)
        with self.assertRaises(cf.ConfigError) as ctx:
            cf.validate_port("abc")
        self.assertEqual(str(ctx.exception), "PORT must be an integer, got 'abc'")
        with self.assertRaises(cf.ConfigError) as ctx:
            cf.validate_port(0)
        self.assertEqual(str(ctx.exception), "PORT must be in 1..65535, got 0")

    def test_build_profile(self):
        self.assertEqual(cf.validate_build_profile({}, "debug"), "debug")
        with self.assertRaises(cf.ConfigError) as ctx:
            cf.validate_build_profile({"BUILD_PROFILE": "prod"}, "release")
        self.assertEqual(str(ctx.exception), "BUILD_PROFILE must be release or debug, got 'prod'")

    def test_out_rules(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = cf.validate_out(str(Path(tmp) / "cell"), fresh=True)
            self.assertEqual(out, Path(tmp).resolve() / "cell")
        with self.assertRaises(cf.ConfigError) as ctx:
            cf.validate_out("", fresh=False)
        self.assertEqual(str(ctx.exception), "OUT is required")
        with self.assertRaises(cf.ConfigError) as ctx:
            cf.validate_out("", fresh=False, required="OUT is required (environment OUT)")
        self.assertEqual(str(ctx.exception), "OUT is required (environment OUT)")
        with self.assertRaises(cf.ConfigError):
            cf.validate_out("/", fresh=False)
        with self.assertRaises(cf.ConfigError):
            cf.validate_out(cf.REPO_ROOT / "cell", fresh=False)

    def test_image_home_src_requires_a_prepared_sandbox(self):
        with tempfile.TemporaryDirectory() as tmp:
            empty = Path(tmp) / "sandbox"
            empty.mkdir()
            with self.assertRaises(cf.ConfigError) as ctx:
                cf.validate_image_home_src(str(empty))
            self.assertIn("sandbox-manifest.json", str(ctx.exception))
            (empty / "sandbox-manifest.json").write_text("{}")
            self.assertEqual(cf.validate_image_home_src(str(empty)), empty)


class CellConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_flags_win_over_the_environment(self):
        cfg = cf.CellConfig.from_env(
            {"OUT": str(self.root / "ignored"), "PORT": "5800"},
            out_flag=str(self.root / "cell"),
            port_flag="5801",
        )
        self.assertEqual(cfg.out, self.root.resolve() / "cell")
        self.assertEqual(cfg.port, 5801)


class WorkloadConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.cell = self.root / "cell"
        self.cell.mkdir()
        (self.cell / "cookies.txt").write_text("jwt\tx\n")
        (self.cell / "launcher-info.json").write_text(
            json.dumps(
                {
                    "base_url": "http://127.0.0.1:9999",
                    "cookie_jar": str(self.cell / "cookies.txt"),
                    "out": str(self.cell),
                    "mode": "bench",
                }
            )
        )
        self.repo = self.root / "repo"
        self.repo.mkdir()

    def env(self, **kw):
        env = {
            "LAUNCHER_INFO": str(self.cell / "launcher-info.json"),
            "IMAGE_REPO": str(self.repo),
        }
        env.update(kw)
        return env

    def test_builds_one_object_with_derived_identity(self):
        cfg = cf.WorkloadConfig.from_env(self.env())
        self.assertEqual(cfg.base_url, "http://127.0.0.1:9999")
        self.assertEqual(cfg.out, self.cell.resolve())
        self.assertEqual(cfg.port, 9999)
        self.assertEqual(cfg.dataset_id, "out-local")

    def test_port_env_is_validated_while_identity_comes_from_launcher_info(self):
        cfg = cf.WorkloadConfig.from_env(self.env(PORT="5800"))
        self.assertEqual(cfg.port, 9999)
        with self.assertRaises(cf.ConfigError):
            cf.WorkloadConfig.from_env(self.env(PORT="abc"))


class BatteryConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_level_defaults_and_exported_overrides(self):
        for images, repeats, timeout in ((50, 5, "1800"), (100, 5, "1800"), (5650, 3, "7200")):
            with self.subTest(images=images):
                cfg = cf.BatteryConfig.from_env({}, images=images, out=self.root / str(images))
                self.assertEqual(cfg.repeats, repeats)
        cfg = cf.BatteryConfig.from_env({}, images=100, out=self.root / "100")
        self.assertEqual(cfg.workload_timeout_s, "1800")
        cfg = cf.BatteryConfig.from_env(
            {"BATTERY_REPEATS": "1", "WORKLOAD_TIMEOUT_S": "999"},
            images=5650,
            out=self.root / "5650",
        )
        self.assertEqual(cfg.repeats, 1)
        self.assertEqual(cfg.workload_timeout_s, "999")

    def test_repeats_are_validated_once(self):
        for raw in ("zero", "0"):
            with self.subTest(raw=raw):
                with self.assertRaises(cf.ConfigError):
                    cf.BatteryConfig.from_env({"BATTERY_REPEATS": raw}, out=self.root / "gate")


class PrepareConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.archive = self.root / "archive"
        self.archive.mkdir()
        (self.archive / "a.jpg").write_bytes(b"x")

    def test_flags_build_the_object(self):
        cfg = cf.PrepareConfig.from_args(
            ["--archive", str(self.archive), "--sandbox", str(self.root / "s"), "--count", "4"]
        )
        self.assertEqual(cfg.archive, self.archive)
        self.assertEqual(cfg.sandbox, self.root / "s")
        self.assertEqual(cfg.count, 4)
        self.assertFalse(cfg.large)

    def test_rejects_missing_archive_and_non_positive_count(self):
        with self.assertRaises(cf.ConfigError):
            cf.PrepareConfig.from_args(
                ["--archive", str(self.root / "nope"), "--sandbox", str(self.root / "s")]
            )
        with self.assertRaises(cf.ConfigError):
            cf.PrepareConfig.from_args(
                [
                    "--archive",
                    str(self.archive),
                    "--sandbox",
                    str(self.root / "s"),
                    "--count",
                    "0",
                ]
            )


class PreflightConfigTest(unittest.TestCase):
    def test_flags_win_and_environment_fills_the_rest(self):
        cfg = cf.PreflightConfig.from_env(
            {"OUT": "/tmp/env-cell", "PORT": "5800", "IMAGE_HOME_SRC": "/tmp/env-sandbox"},
            out="/tmp/flag-cell",
        )
        self.assertEqual(cfg.out, "/tmp/flag-cell")
        self.assertEqual(cfg.port, "5800")
        self.assertEqual(cfg.sandbox, "/tmp/env-sandbox")

    def test_absent_optional_inputs_stay_none(self):
        cfg = cf.PreflightConfig.from_env({})
        self.assertIsNone(cfg.out)
        self.assertIsNone(cfg.archive)
        self.assertIsNone(cfg.sandbox)
        self.assertEqual(cfg.port, cf.DEFAULT_PORT)


class OneValidationThreeConsumersTest(unittest.TestCase):
    """A bad environment value is rejected once and reported identically."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def launcher_env(self, port):
        return {
            "BUILD_PROFILE": "release",
            "PICASU_BIN": sys.executable,
            "OUT": str(self.root / "cell"),
            "PORT": port,
        }

    def cell_env(self, port):
        return {
            "OUT": str(self.root / "cell"),
            "PORT": port,
        }

    def workload_env(self, port):
        cell = self.root / "wcell"
        cell.mkdir(exist_ok=True)
        (cell / "cookies.txt").write_text("jwt\tx\n")
        (cell / "launcher-info.json").write_text(
            json.dumps(
                {
                    "base_url": "http://127.0.0.1:9999",
                    "cookie_jar": str(cell / "cookies.txt"),
                    "out": str(cell),
                    "mode": "bench",
                }
            )
        )
        repo = self.root / "repo"
        repo.mkdir(exist_ok=True)
        return {
            "LAUNCHER_INFO": str(cell / "launcher-info.json"),
            "IMAGE_REPO": str(repo),
            "PORT": port,
        }

    def _rejection_messages(self, bad_port, extra=None):
        envs = (self.launcher_env(bad_port), self.cell_env(bad_port), self.workload_env(bad_port))
        if extra is not None:
            envs = tuple(env | extra for env in envs)
        factories = (bl.config_from_env, oc.config_from_env, workload.config_from_env)
        messages = []
        for factory, env in zip(factories, envs, strict=True):
            with self.assertRaises(cf.ConfigError) as caught:
                factory(env)
            messages.append(str(caught.exception))
        return messages

    def test_bad_port_is_reported_identically_by_all_three_layers(self):
        for bad in ("abc", "0", "70000"):
            with self.subTest(port=bad):
                messages = self._rejection_messages(bad)
                self.assertTrue(all(messages))
                self.assertEqual(len(set(messages)), 1)

    def test_bad_build_profile_is_reported_identically_by_all_three_layers(self):
        messages = self._rejection_messages("5800", extra={"BUILD_PROFILE": "prod"})
        self.assertTrue(all(messages))
        self.assertEqual(len(set(messages)), 1)

    def test_preflight_reports_the_shared_port_rule(self):
        check = pf.check_port("abc")
        self.assertFalse(check.ok)
        with self.assertRaises(cf.ConfigError) as caught:
            cf.validate_port("abc")
        self.assertEqual(check.detail, str(caught.exception))


if __name__ == "__main__":
    unittest.main()


class MinValidRoundsConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.cell = self.root / "cell"
        self.cell.mkdir()
        (self.cell / "cookies.txt").write_text("jwt\tx\n")
        (self.cell / "launcher-info.json").write_text(
            json.dumps(
                {
                    "base_url": "http://127.0.0.1:9999",
                    "cookie_jar": str(self.cell / "cookies.txt"),
                    "out": str(self.cell),
                    "mode": "bench",
                }
            )
        )
        self.repo = self.root / "repo"
        self.repo.mkdir()

    def env(self, **kw):
        env = {
            "LAUNCHER_INFO": str(self.cell / "launcher-info.json"),
            "IMAGE_REPO": str(self.repo),
        }
        env.update(kw)
        return env

    def test_min_valid_rounds_default(self):
        cfg = cf.WorkloadConfig.from_env(self.env())
        self.assertEqual(cfg.min_valid_rounds, 3)

    def test_min_valid_rounds_override(self):
        cfg = cf.WorkloadConfig.from_env(self.env(MIN_VALID_ROUNDS="5"))
        self.assertEqual(cfg.min_valid_rounds, 5)

    def test_min_valid_rounds_invalid(self):
        for value in ("0", "abc"):
            with self.subTest(value=value):
                with self.assertRaises(cf.ConfigError):
                    cf.WorkloadConfig.from_env(self.env(MIN_VALID_ROUNDS=value))


class ProbeRefreshConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.cell = self.root / "cell"
        self.cell.mkdir()
        (self.cell / "cookies.txt").write_text("jwt\tx\n")
        (self.cell / "launcher-info.json").write_text(
            json.dumps(
                {
                    "base_url": "http://127.0.0.1:9999",
                    "cookie_jar": str(self.cell / "cookies.txt"),
                    "out": str(self.cell),
                    "mode": "bench",
                }
            )
        )
        self.repo = self.root / "repo"
        self.repo.mkdir()

    def env(self, **kw):
        env = {
            "LAUNCHER_INFO": str(self.cell / "launcher-info.json"),
            "IMAGE_REPO": str(self.repo),
        }
        env.update(kw)
        return env

    def test_probe_refresh_default(self):
        cfg = cf.WorkloadConfig.from_env(self.env())
        self.assertEqual(cfg.probe_refresh_s, 0.0)

    def test_probe_refresh_override(self):
        cfg = cf.WorkloadConfig.from_env(self.env(PROBE_REFRESH_S="0.5"))
        self.assertEqual(cfg.probe_refresh_s, 0.5)

    def test_probe_refresh_zero_valid(self):
        cfg = cf.WorkloadConfig.from_env(self.env(PROBE_REFRESH_S="0"))
        self.assertEqual(cfg.probe_refresh_s, 0.0)

    def test_probe_refresh_invalid(self):
        for value in ("-1", "abc"):
            with self.subTest(value=value):
                with self.assertRaises(cf.ConfigError):
                    cf.WorkloadConfig.from_env(self.env(PROBE_REFRESH_S=value))


class ScanWarmupConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.cell = self.root / "cell"
        self.cell.mkdir()
        (self.cell / "cookies.txt").write_text("jwt\tx\n")
        (self.cell / "launcher-info.json").write_text(
            json.dumps(
                {
                    "base_url": "http://127.0.0.1:9999",
                    "cookie_jar": str(self.cell / "cookies.txt"),
                    "out": str(self.cell),
                    "mode": "bench",
                }
            )
        )
        self.repo = self.root / "repo"
        self.repo.mkdir()

    def env(self, **kw):
        env = {
            "LAUNCHER_INFO": str(self.cell / "launcher-info.json"),
            "IMAGE_REPO": str(self.repo),
        }
        env.update(kw)
        return env

    def test_scan_warmup_default(self):
        cfg = cf.WorkloadConfig.from_env(self.env())
        self.assertEqual(cfg.scan_warmup, 1)

    def test_scan_warmup_zero(self):
        cfg = cf.WorkloadConfig.from_env(self.env(SCAN_WARMUP="0"))
        self.assertEqual(cfg.scan_warmup, 0)

    def test_scan_warmup_negative_invalid(self):
        with self.assertRaises(cf.ConfigError):
            cf.WorkloadConfig.from_env(self.env(SCAN_WARMUP="-1"))


class MinProbeRoundsConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_default(self):
        cfg = cf.BatteryConfig.from_env({}, out=self.root / "gate")
        self.assertEqual(cfg.min_probe_rounds, 20)

    def test_override(self):
        cfg = cf.BatteryConfig.from_env({"MIN_PROBE_ROUNDS": "7"}, out=self.root / "gate")
        self.assertEqual(cfg.min_probe_rounds, 7)

    def test_invalid(self):
        for raw in ("zero", "0"):
            with self.subTest(raw=raw):
                with self.assertRaises(cf.ConfigError):
                    cf.BatteryConfig.from_env({"MIN_PROBE_ROUNDS": raw}, out=self.root / "gate")


class WorkloadProfileTest(unittest.TestCase):
    """Every scenario profile is accepted; unknown labels are refused."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        cell = root / "cell"
        cell.mkdir()
        (cell / "cookies.txt").write_text("jwt\tx\n")
        (cell / "launcher-info.json").write_text(
            json.dumps(
                {
                    "base_url": "http://127.0.0.1:9999",
                    "cookie_jar": str(cell / "cookies.txt"),
                    "out": str(cell),
                }
            )
        )
        repo = root / "repo"
        repo.mkdir()
        self.base = {
            "LAUNCHER_INFO": str(cell / "launcher-info.json"),
            "IMAGE_REPO": str(repo),
        }

    def test_every_declared_profile_is_accepted(self):
        import scenarios

        for profile in cf.WORKLOADS:
            with self.subTest(profile=profile):
                cfg = cf.WorkloadConfig.from_env(dict(self.base, WORKLOAD=profile))
                self.assertEqual(cfg.workload, profile)
                self.assertTrue(scenarios.is_storm(profile) == scenarios.is_storm(cfg.workload))

    def test_storm_clients_derive_from_the_level(self):
        import scenarios

        for profile in scenarios.storm_profiles():
            with self.subTest(profile=profile):
                _background, _kind, level = scenarios.storm_parts(profile)
                cfg = cf.WorkloadConfig.from_env(dict(self.base, WORKLOAD=profile))
                self.assertEqual(cfg.storm_clients, scenarios.level_clients(level, cfg.ncpu))

    def test_unknown_profile_is_refused(self):
        with self.assertRaises(cf.ConfigError):
            cf.WorkloadConfig.from_env(dict(self.base, WORKLOAD="not-a-declared-profile"))


class StormMinCoverageConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.cell = self.root / "cell"
        self.cell.mkdir()
        (self.cell / "cookies.txt").write_text("jwt\tx\n")
        (self.cell / "launcher-info.json").write_text(
            __import__("json").dumps(
                {
                    "base_url": "http://127.0.0.1:9999",
                    "cookie_jar": str(self.cell / "cookies.txt"),
                    "out": str(self.cell),
                    "mode": "bench",
                }
            )
        )
        self.repo = self.root / "repo"
        self.repo.mkdir()

    def env(self, **kw):
        env = {
            "LAUNCHER_INFO": str(self.cell / "launcher-info.json"),
            "IMAGE_REPO": str(self.repo),
        }
        env.update(kw)
        return env

    def test_default(self):
        cfg = cf.WorkloadConfig.from_env(self.env())
        self.assertEqual(cfg.storm_min_coverage_pct, 50.0)

    def test_override(self):
        cfg = cf.WorkloadConfig.from_env(self.env(STORM_MIN_COVERAGE_PCT="75"))
        self.assertEqual(cfg.storm_min_coverage_pct, 75.0)

    def test_invalid(self):
        for raw in ("0", "101", "abc"):
            with self.subTest(raw=raw):
                with self.assertRaises(cf.ConfigError):
                    cf.WorkloadConfig.from_env(self.env(STORM_MIN_COVERAGE_PCT=raw))


class ScenarioProfilesTest(unittest.TestCase):
    """The workload set is the scenario table, with one probe cell."""

    def test_workloads_are_the_scenario_profiles(self):
        import scenarios

        self.assertEqual(cf.WORKLOADS, scenarios.profiles())
        for profile in (scenarios.PROBE, scenarios.IDLE_UPLOAD, scenarios.SCAN_UPLOAD):
            self.assertIn(profile, cf.WORKLOADS)

    def test_workload_min_probe_rounds_default_and_override(self):
        import tempfile
        from pathlib import Path

        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        cell = root / "cell"
        cell.mkdir()
        (cell / "cookies.txt").write_text("jwt\tx\n")
        (cell / "launcher-info.json").write_text(
            json.dumps(
                {
                    "base_url": "http://127.0.0.1:9999",
                    "cookie_jar": str(cell / "cookies.txt"),
                    "out": str(cell),
                }
            )
        )
        repo = root / "repo"
        repo.mkdir()
        base = {"LAUNCHER_INFO": str(cell / "launcher-info.json"), "IMAGE_REPO": str(repo)}
        self.assertEqual(cf.WorkloadConfig.from_env(dict(base)).min_probe_rounds, 20)
        self.assertEqual(
            cf.WorkloadConfig.from_env(dict(base, MIN_PROBE_ROUNDS="7")).min_probe_rounds, 7
        )
