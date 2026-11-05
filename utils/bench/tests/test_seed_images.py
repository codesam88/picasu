#!/usr/bin/env python3
"""Tests for seed_images.py. Stdlib only; no real snapfab required."""

import importlib.util
import json
import os
import stat
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent

FAKE_SNAPFAB = (
    "#!/usr/bin/env python3\n"
    "import json, sys\n"
    "entries = json.load(open(sys.argv[-1]))\n"
    "for entry in entries:\n"
    "    open(entry['output'], 'wb').write(b'x' * 1000)\n"
)


def load_seed():
    spec = importlib.util.spec_from_file_location("seed_images", BENCH / "seed_images.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def write_executable(path: Path, text: str) -> Path:
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


class SeedTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.tmp_path = Path(self.tmp.name)
        self.snapfab = write_executable(self.tmp_path / "fake-snapfab", FAKE_SNAPFAB)
        self.seed_mod = load_seed()
        self.seed_main = self.seed_mod.main

    def seed(self, *args):
        argv = ["--snapfab", str(self.snapfab), *args]
        self.assertEqual(self.seed_main(argv), 0)

    def test_synth_entries_shape(self):
        entries = self.seed_mod.synth_entries(3)
        self.assertEqual(
            [e["output"] for e in entries], ["img_00001.jpg", "img_00002.jpg", "img_00003.jpg"]
        )
        for entry in entries:
            self.assertEqual(
                (entry["format"], entry["width"], entry["height"]),
                ("jpeg", 4000, 3000),
            )

    def test_seed_writes_manifest_and_images(self):
        repo = self.tmp_path / "repo"
        self.seed(*["--repo", str(repo), "--count", "5"])
        manifest = json.loads((repo / "manifest.json").read_text())
        self.assertEqual(len(manifest), 5)
        got = sorted(p.name for p in repo.glob("*.jpg"))
        self.assertEqual(got, [e["output"] for e in manifest])

    def test_template_subset(self):
        template = self.tmp_path / "template.json"
        template.write_text(
            json.dumps([{"output": f"t_{i:02d}.jpg", "format": "jpeg"} for i in range(10)])
        )
        repo = self.tmp_path / "repo"
        self.seed(*["--repo", str(repo), "--count", "4", "--template", str(template)])
        got = sorted(p.name for p in repo.glob("*.jpg"))
        self.assertEqual(got, [f"t_{i:02d}.jpg" for i in range(4)])

    def test_short_template_fails(self):
        template = self.tmp_path / "template.json"
        template.write_text(json.dumps([{"output": "a.jpg"}]))
        repo = self.tmp_path / "repo"
        with self.assertRaises(SystemExit):
            self.seed(*["--repo", str(repo), "--count", "4", "--template", str(template)])

    def test_nonempty_repo_refuses_without_overwrite(self):
        repo = self.tmp_path / "repo"
        repo.mkdir()
        (repo / "img_00001.jpg").write_bytes(b"x")
        with self.assertRaises(SystemExit):
            self.seed(*["--repo", str(repo), "--count", "2"])

    def test_overwrite_reseeds(self):
        repo = self.tmp_path / "repo"
        repo.mkdir()
        (repo / "stale.jpg").write_bytes(b"x")
        old = os.environ.get("OVERWRITE")
        os.environ["OVERWRITE"] = "1"
        try:
            self.seed(*["--repo", str(repo), "--count", "2"])
        finally:
            if old is None:
                del os.environ["OVERWRITE"]
            else:
                os.environ["OVERWRITE"] = old
        self.assertFalse((repo / "stale.jpg").exists())
        self.assertEqual(len(list(repo.glob("*.jpg"))), 2)

    def test_missing_snapfab_fails(self):
        repo = self.tmp_path / "repo"
        with self.assertRaises(SystemExit):
            self.seed(
                *["--repo", str(repo), "--count", "2", "--snapfab", str(self.tmp_path / "missing")]
            )


if __name__ == "__main__":
    unittest.main()
