#!/usr/bin/env python3
"""Tests for the bench justfile recipes. Requires `just`.

The recipes are shell, so they are exercised through `just` itself:

- the one-line aliases are checked with `--dry-run`, which expands the
  recipe without running it (the bench.py behaviour they forward to is
  covered in `test_bench_cli.py`);
- the workload recipe runs for real, with its interpreter replaced by a
  stub that records the arguments and environment a cell would get
  instead of launching one.

Both need `just` on PATH and the venv the `_venv` recipe maintains (it is
already created when the suite runs through `just bench test`), so the
class is skipped without them.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
JUSTFILE = BENCH / "justfile"
JUST = shutil.which("just")
VENV_RUFF = BENCH / ".venv" / "bin" / "ruff"

STUB = """#!/usr/bin/env bash
# Test double for the bench venv python: the sandbox check runs for real
# (it reads the program from stdin), everything else is recorded.
case "${1:-}" in
    -|-c) shift; exec "$BENCH_TEST_REAL_PYTHON" "$@" ;;
esac
{
    printf 'argv=%s\\n' "$*"
    printf 'IMAGE_HOME_SRC=%s\\n' "${IMAGE_HOME_SRC-}"
    printf 'OUT=%s\\n' "${OUT-}"
} > "$BENCH_TEST_RECORD"
exit 0
"""


@unittest.skipUnless(
    JUST and VENV_RUFF.is_file(),
    "needs `just` and the bench venv (run the suite via `just bench test`)",
)
class RecipeTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.stub = self.tmp / "stub-python"
        self.stub.write_text(STUB)
        self.stub.chmod(0o755)
        self.record = self.tmp / "record"
        self.sandbox = self.tmp / "sandbox"
        self.sandbox.mkdir()
        self.out = self.tmp / "cell-out"
        self.repo = self.tmp / "set-100"

    def just(self, *args, env=None):
        environ = dict(os.environ)
        environ.pop("IMAGE_HOME_SRC", None)
        environ.update(
            {
                "BENCH_TEST_REAL_PYTHON": sys.executable,
                "BENCH_TEST_RECORD": str(self.record),
            }
        )
        if env:
            environ.update(env)
        return subprocess.run(
            [JUST, "-f", str(JUSTFILE), *args],
            capture_output=True,
            text=True,
            env=environ,
            cwd=str(BENCH),
            check=False,
        )

    def run_workload(self, image_home_src=None):
        env = {} if image_home_src is None else {"IMAGE_HOME_SRC": image_home_src}
        return self.just(
            "--set",
            "venv_python",
            str(self.stub),
            "workload",
            str(self.out),
            str(self.repo),
            env=env,
        )

    def recorded(self):
        self.assertTrue(self.record.is_file(), "the workload recipe never reached the cell")
        return self.record.read_text()

    def test_status_recipe_forwards_to_the_unified_cli(self):
        run = self.just("--dry-run", "status", "/tmp/picasu-battery-gate")

        # just prints a dry run to stderr.
        self.assertEqual(run.returncode, 0, run.stdout)
        self.assertIn('bench.py" status /tmp/picasu-battery-gate', run.stderr)

    def test_workload_refuses_a_sandbox_without_a_manifest(self):
        run = self.run_workload(image_home_src=str(self.sandbox))

        self.assertEqual(run.returncode, 1, run.stdout)
        self.assertIn("FAIL sandbox:", run.stderr)
        self.assertIn("sandbox-manifest.json", run.stderr)
        self.assertIn("prepare it with `bench prepare`", run.stderr)
        # Fail fast: no cell process was started.
        self.assertFalse(self.record.exists())
        self.assertFalse((self.out / "report.tsv").exists())

    def test_workload_forwards_a_prepared_sandbox_into_the_cell_env(self):
        (self.sandbox / "sandbox-manifest.json").write_text(json.dumps({"count": 100}))

        run = self.run_workload(image_home_src=str(self.sandbox))

        self.assertEqual(run.returncode, 0, run.stderr)
        recorded = self.recorded()
        self.assertIn(f"IMAGE_HOME_SRC={self.sandbox}\n", recorded)
        self.assertIn(f"OUT={self.out}\n", recorded)
        self.assertIn(f"run cell --out {self.out}", recorded)

    def test_workload_without_a_sandbox_is_unchanged(self):
        run = self.run_workload()

        self.assertEqual(run.returncode, 0, run.stderr)
        recorded = self.recorded()
        self.assertIn(f"run cell --out {self.out}", recorded)
        self.assertIn("IMAGE_HOME_SRC=\n", recorded)

    def test_extract_recipe_runs_battery_extract(self):
        run = self.just("--dry-run", "extract", "/tmp/picasu-battery-gate")

        self.assertEqual(run.returncode, 0, run.stdout)
        self.assertIn('battery.py" extract "/tmp/picasu-battery-gate"', run.stderr)

    def test_extract_recipe_forwards_an_output_path(self):
        run = self.just("--dry-run", "extract", "/tmp/picasu-battery-gate", "/tmp/out.json")

        self.assertEqual(run.returncode, 0, run.stdout)
        self.assertIn('battery.py" extract "/tmp/picasu-battery-gate"', run.stderr)
        self.assertIn('--out "/tmp/out.json"', run.stderr)

    def test_extract_recipe_reaches_battery_extract_through_the_stub(self):
        run = self.just(
            "--set",
            "venv_python",
            str(self.stub),
            "extract",
            str(self.tmp / "gate-run"),
        )

        self.assertEqual(run.returncode, 0, run.stderr)
        recorded = self.recorded()
        self.assertIn("battery.py", recorded)
        self.assertIn("extract", recorded)


if __name__ == "__main__":
    unittest.main()
