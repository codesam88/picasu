#!/usr/bin/env python3
"""Tests for prepare.py. Stdlib only."""

import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

BENCH = Path(__file__).resolve().parent.parent


def load_prepare():
    spec = importlib.util.spec_from_file_location("prepare", BENCH / "prepare.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class PrepareTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.tmp_path = Path(self.tmp.name)
        self.archive = self.tmp_path / "archive"
        self.archive.mkdir()
        for i in range(5):
            (self.archive / f"img_{i:04d}.jpg").write_bytes(b"x" * 100)
        self.prepare = load_prepare()

    def test_happy_path_link_mode_and_manifest(self):
        sandbox = self.tmp_path / "sandbox"
        rc = self.prepare.main(["--archive", str(self.archive), "--sandbox", str(sandbox), "--count", "3"])
        self.assertEqual(rc, 0)
        self.assertTrue((sandbox / "sandbox-manifest.json").is_file())
        manifest = json.loads((sandbox / "sandbox-manifest.json").read_text())
        self.assertEqual(manifest["archive"], str(self.archive))
        self.assertEqual(manifest["count"], 3)
        self.assertEqual(manifest["bytes"], 300)
        self.assertIn(manifest["linkage"], ("linked", "copied", "mixed"))
        self.assertEqual(len(list(sandbox.glob("*.jpg"))), 3)
        got = sorted(p.name for p in sandbox.glob("*.jpg"))
        self.assertEqual(got, ["img_0000.jpg", "img_0001.jpg", "img_0002.jpg"])

    def test_copy_fallback_via_mock(self):
        sandbox = self.tmp_path / "sandbox"
        with mock.patch("os.link", side_effect=OSError("no link")):
            rc = self.prepare.main(["--archive", str(self.archive), "--sandbox", str(sandbox), "--count", "2"])
        self.assertEqual(rc, 0)
        manifest = json.loads((sandbox / "sandbox-manifest.json").read_text())
        self.assertEqual(manifest["linkage"], "copied")

    def test_refuse_non_empty(self):
        sandbox = self.tmp_path / "sandbox"
        sandbox.mkdir()
        (sandbox / "stale.jpg").write_bytes(b"x")
        rc = self.prepare.main(["--archive", str(self.archive), "--sandbox", str(sandbox), "--count", "2"])
        self.assertEqual(rc, 1)

    def test_missing_archive(self):
        sandbox = self.tmp_path / "sandbox"
        rc = self.prepare.main(["--archive", str(self.tmp_path / "nope"), "--sandbox", str(sandbox), "--count", "2"])
        self.assertEqual(rc, 1)

    def test_force_allows_non_empty(self):
        sandbox = self.tmp_path / "sandbox"
        sandbox.mkdir()
        (sandbox / "stale.jpg").write_bytes(b"x")
        rc = self.prepare.main(["--archive", str(self.archive), "--sandbox", str(sandbox), "--count", "2", "--force"])
        self.assertEqual(rc, 0)
        self.assertFalse((sandbox / "stale.jpg").exists())


if __name__ == "__main__":
    unittest.main()
