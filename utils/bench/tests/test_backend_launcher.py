#!/usr/bin/env python3
"""Tests for backend_launcher.py core. Stdlib only: python3 -m unittest -v.

Covers: config validation and bench/debug guards (pure), FRESH/reuse
requirements, manifest identity, and preflight rules. Startup and
supervision paths run live cells instead of test doubles.
"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))

import backend_launcher  # noqa: E402


def load_launcher():
    return backend_launcher


bl = load_launcher()


def base_env(**kw):
    e = {
        "BUILD_PROFILE": "release",
        "PICASU_BIN": sys.executable,
        "GDB": "0",
    }
    e.update(kw)
    return e


class LauncherTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def test_bench_defaults(self):
        env = base_env(OUT=str(Path(self.tmp.name) / "c"))
        env.pop("GDB")
        cfg = bl.config_from_env(env)
        self.assertEqual(cfg.mode, "bench")
        self.assertTrue(cfg.fresh)
        self.assertEqual(cfg.build_profile, "release")
        self.assertEqual(cfg.ready_timeout, 120)
        self.assertEqual(cfg.launch_timeout, 0)
        self.assertEqual(cfg.ncpu, 4)
        self.assertTrue(cfg.gdb)
        self.assertEqual(cfg.admin_id, "bench-admin")
        self.assertTrue(cfg.admin_pw.startswith("bench-"))

    def test_out_refusals(self):
        for bad in ("", "/", str(bl.REPO_ROOT), str(bl.REPO_ROOT) + "/x"):
            with self.assertRaises(bl.ConfigError, msg=bad):
                bl.config_from_env(base_env(OUT=bad))

    def test_out_outside_tmp_refused(self):
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(OUT="/var/lib/picasu-launcher-test"))

    def test_mode_derived_from_profile_and_env(self):
        out = str(Path(self.tmp.name) / "c")
        cfg = bl.config_from_env(base_env(OUT=out))
        self.assertEqual(cfg.mode, "bench")
        cfg = bl.config_from_env(base_env(OUT=out, RUST_LOG="info"))
        self.assertEqual(cfg.mode, "debug")
        cfg = bl.config_from_env(base_env(OUT=out, BUILD_PROFILE="debug"))
        self.assertEqual(cfg.mode, "debug")

    def test_missing_binary(self):
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(
                base_env(OUT=str(Path(self.tmp.name) / "c"), PICASU_BIN="/nonexistent")
            )

    def test_bad_port_and_fresh(self):
        out = str(Path(self.tmp.name) / "c")
        for bad in ("abc", "0", "70000"):
            with self.assertRaises(bl.ConfigError, msg=bad):
                bl.config_from_env(base_env(OUT=out, PORT=bad))
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(OUT=out, FRESH="yes"))

    def test_reuse_requires_data_and_password(self):
        out = Path(self.tmp.name) / "cell"
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(OUT=str(out), FRESH="0", ADMIN_PW="pw"))
        (out / "data").mkdir(parents=True)
        with self.assertRaises(bl.ConfigError):
            bl.config_from_env(base_env(OUT=str(out), FRESH="0"))
        cfg = bl.config_from_env(base_env(OUT=str(out), FRESH="0", ADMIN_PW="pw"))
        self.assertFalse(cfg.fresh)


class ImageHomeAndManifestTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_write_launcher_info_includes_image_home(self):
        src = self.root / "src"
        src.mkdir()
        (src / "a.jpg").write_bytes(b"x")
        (src / "sandbox-manifest.json").write_text(json.dumps({"archive": str(src), "count": 1}))
        out = self.root / "cell"
        env = base_env(OUT=str(out), IMAGE_HOME_SRC=str(src))
        cfg = bl.config_from_env(env)
        bl.prepare_out(cfg)
        bl.write_launcher_info(cfg, 4321)
        info = json.loads((out / "launcher-info.json").read_text())
        self.assertEqual(info["image_home"], str(src))
        self.assertEqual(info["pid"], 4321)

    def test_image_home_src_override_no_images_dir_and_env(self):
        src = self.root / "src"
        src.mkdir()
        (src / "a.jpg").write_bytes(b"x")
        (src / "b.jpg").write_bytes(b"x")
        (src / "sandbox-manifest.json").write_text(json.dumps({"archive": str(src), "count": 2}))
        out = self.root / "cell"
        env = base_env(OUT=str(out), IMAGE_HOME_SRC=str(src), DATASET_ID="ds-1")
        cfg = bl.config_from_env(env)
        self.assertEqual(cfg.image_home, src)
        bl.prepare_out(cfg)
        self.assertFalse((out / "images").exists())
        self.assertTrue(src.exists())
        self.assertEqual(sorted(p.name for p in src.glob("*.jpg")), ["a.jpg", "b.jpg"])
        se = bl.server_env(cfg, env)
        self.assertEqual(se["PICASU_IMAGE_HOME"], str(src))
        bl.write_manifest(cfg, env)
        m = json.loads((out / "run-manifest.json").read_text())
        self.assertEqual(m["dataset"]["count"], 2)
        self.assertEqual(m["dataset"]["id"], "ds-1")
        self.assertEqual(m["dataset"]["source"], str(src))
        self.assertIn("cpu_model", m)
        self.assertIn("mem_total", m)
        self.assertIn("fs_type", m)
        self.assertIn("binary_sha256", m)
        self.assertIn("PICASU_OBSERVER_STATS", m)

    def test_observer_flag_recording_on_off(self):
        for val, expected in ((None, "1"), ("1", "1"), ("0", "0")):
            out = self.root / f"cell-{val}"
            env = base_env(OUT=str(out))
            if val is not None:
                env["PICASU_OBSERVER_STATS"] = val
            cfg = bl.config_from_env(env)
            bl.prepare_out(cfg)
            bl.write_manifest(cfg, env)
            m = json.loads((cfg.out / "run-manifest.json").read_text())
            self.assertEqual(m["PICASU_OBSERVER_STATS"], expected)
            self.assertEqual(m["observer_stats"], expected)
            self.assertEqual(m["effective_observer_stats"], expected)

    def test_local_dataset_recorded_as_out_local(self):
        out = self.root / "cell"
        (out / "data").mkdir(parents=True)
        (out / "images").mkdir(parents=True)
        (out / "images" / "x.jpg").write_bytes(b"x")
        env = base_env(OUT=str(out), FRESH="0", ADMIN_PW="pw")
        cfg = bl.config_from_env(env)
        bl.prepare_out(cfg)
        bl.write_manifest(cfg, env)
        m = json.loads((out / "run-manifest.json").read_text())
        self.assertEqual(m["dataset"]["source"], "out-local")
        self.assertEqual(m["dataset"]["count"], 1)
        self.assertEqual(m["dataset"]["id"], "out-local")


if __name__ == "__main__":
    unittest.main()
