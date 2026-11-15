#!/usr/bin/env python3
"""Tests for config.py: the shared validated configuration.

The per-layer suites pin every layer's accepted and rejected values; this
file pins the shared validators and the one-error-shape contract: the
launcher, the cell orchestrator and the workload reject the same bad
environment value through the same rule, with the same message.
"""

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent


def load_module(name: str, filename: str):
    """Load one bench module under its canonical name, once per process."""
    module = sys.modules.get(name)
    if module is not None:
        return module
    spec = importlib.util.spec_from_file_location(name, BENCH / filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


cf = load_module("config", "config.py")
bl = load_module("backend_launcher", "backend_launcher.py")
oc = load_module("run_cell", "run-cell.py")
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
            {"OUT": str(self.root / "ignored"), "PORT": "5800", "HYPERFINE_BIN": sys.executable},
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
            "HYPERFINE_BIN": sys.executable,
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
        cfg = cf.BatteryConfig.from_env({}, level="gate", out=self.root / "gate")
        self.assertEqual(cfg.repeats, 3)
        self.assertEqual(cfg.workload_timeout_s, "1800")
        cfg = cf.BatteryConfig.from_env(
            {"BATTERY_REPEATS": "1", "WORKLOAD_TIMEOUT_S": "999"},
            level="scale",
            out=self.root / "scale",
        )
        self.assertEqual(cfg.repeats, 1)
        self.assertEqual(cfg.workload_timeout_s, "999")

    def test_repeats_are_validated_once(self):
        for raw in ("zero", "0"):
            with self.subTest(raw=raw):
                with self.assertRaises(cf.ConfigError):
                    cf.BatteryConfig.from_env(
                        {"BATTERY_REPEATS": raw}, level="gate", out=self.root / "gate"
                    )


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
        self.assertTrue(cfg.require_hyperfine)

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
            "HYPERFINE_BIN": sys.executable,
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
            "HYPERFINE_BIN": sys.executable,
            "PORT": port,
        }

    def test_same_bad_port_same_message(self):
        for bad, expected in (
            ("abc", "PORT must be an integer, got 'abc'"),
            ("0", "PORT must be in 1..65535, got 0"),
            ("70000", "PORT must be in 1..65535, got 70000"),
        ):
            with self.subTest(port=bad):
                with self.assertRaises(cf.ConfigError) as launcher:
                    bl.config_from_env(self.launcher_env(bad))
                with self.assertRaises(cf.ConfigError) as cell:
                    oc.config_from_env(self.cell_env(bad))
                with self.assertRaises(cf.ConfigError) as workload_cfg:
                    workload.config_from_env(self.workload_env(bad))
                self.assertEqual(str(launcher.exception), expected)
                self.assertEqual(str(cell.exception), expected)
                self.assertEqual(str(workload_cfg.exception), expected)

    def test_same_bad_profile_same_message(self):
        expected = "BUILD_PROFILE must be release or debug, got 'prod'"
        with self.assertRaises(cf.ConfigError) as launcher:
            bl.config_from_env(self.launcher_env("5800") | {"BUILD_PROFILE": "prod"})
        with self.assertRaises(cf.ConfigError) as cell:
            oc.config_from_env(self.cell_env("5800") | {"BUILD_PROFILE": "prod"})
        with self.assertRaises(cf.ConfigError) as workload_cfg:
            workload.config_from_env(self.workload_env("5800") | {"BUILD_PROFILE": "prod"})
        self.assertEqual(str(launcher.exception), expected)
        self.assertEqual(str(cell.exception), expected)
        self.assertEqual(str(workload_cfg.exception), expected)

    def test_preflight_reports_the_shared_port_rule(self):
        check = pf.check_port("abc")
        self.assertFalse(check.ok)
        self.assertEqual(check.detail, "PORT must be an integer, got 'abc'")


if __name__ == "__main__":
    unittest.main()
