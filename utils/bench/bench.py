#!/usr/bin/env python3
"""Unified CLI shell for the benchmark harness."""

from __future__ import annotations

import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

SUBCOMMAND_NAMES = (
    "run",
    "battery",
    "compare",
    "extract",
    "seed",
    "server",
    "prepare",
    "preflight",
    "status",
)


import backend_launcher  # noqa: E402
import battery  # noqa: E402
import compare_reports  # noqa: E402
import extract_bench  # noqa: E402
import preflight  # noqa: E402
import prepare  # noqa: E402
import seed_images  # noqa: E402


def _read_json(path: Path):
    try:
        return json.loads(path.read_text())
    except OSError, json.JSONDecodeError, ValueError:
        return None


def _rc_from_json(data) -> object:
    if not isinstance(data, dict):
        return None
    for key in ("rc", "exit_code", "exitCode", "workload_rc", "status_rc"):
        if key in data:
            value = data[key]
            try:
                return int(value)
            except TypeError, ValueError:
                return value
    return None


def _classify_cell(cell_dir: Path):
    diag = cell_dir / "diag-orchestrator"
    last = cell_dir / "status-last.json"
    final = cell_dir / "status-final.json"

    if diag.exists():
        return "failed", None

    if final.is_file():
        if last.is_file():
            try:
                if last.stat().st_mtime > final.stat().st_mtime:
                    return "running", None
            except OSError:
                pass
        return "finished", _rc_from_json(_read_json(final))

    if last.is_file():
        return "running", None

    try:
        if any(cell_dir.iterdir()):
            return "failed", None
    except OSError:
        pass
    return "queued", None


def _status(argv) -> int:
    parent = None
    for token in argv or []:
        if not str(token).startswith("-"):
            parent = token
            break
    if parent is None:
        print("usage: bench status <parent>", file=sys.stderr)
        return 2

    parent_path = Path(parent)
    if not parent_path.exists():
        print(f"ERROR: {parent_path}: cannot read parent directory", file=sys.stderr)
        return 1
    if not parent_path.is_dir():
        print(f"ERROR: {parent_path}: not a directory", file=sys.stderr)
        return 1
    # Validate the parent the same way the battery does: a non-battery
    # directory (foreign entries) is refused instead of being walked.
    try:
        foreign = battery.validate_parent(parent_path, fresh=False)
    except Exception as err:  # ConfigError or any other validation failure
        print(f"ERROR: {err}", file=sys.stderr)
        return 1
    if foreign:
        print(
            f"ERROR: {parent_path}: refusing to read a non-battery directory; "
            f"foreign entries: {', '.join(foreign)}",
            file=sys.stderr,
        )
        return 1

    try:
        entries = sorted(
            (entry for entry in parent_path.iterdir() if entry.is_dir()),
            key=lambda p: p.name,
        )
    except OSError:
        print(f"ERROR: {parent_path}: cannot read parent directory", file=sys.stderr)
        return 1

    if not entries:
        print("no cells yet")
        return 0

    printed = 0
    for cell in entries:
        state, rc = _classify_cell(cell)
        if state == "finished" and rc is not None:
            print(f"{cell.name}: finished rc={rc}")
        else:
            print(f"{cell.name}: {state}")
        printed += 1

    return 0 if printed > 0 else 1


def _print_usage() -> None:
    print(
        "usage: bench <subcommand> [args...]\nsubcommands: " + ", ".join(SUBCOMMAND_NAMES),
        file=sys.stderr,
    )


def main(argv=None) -> int:
    if argv is None:
        argv = sys.argv[1:]
    argv = list(argv)
    if not argv:
        _print_usage()
        return 2
    cmd = argv[0]
    rest = argv[1:]

    if cmd in ("-h", "--help"):
        _print_usage()
        return 0
    if cmd not in SUBCOMMAND_NAMES:
        print(f"bench: unknown subcommand: {cmd}", file=sys.stderr)
        _print_usage()
        return 2

    if cmd == "status":
        return _status(rest)

    if cmd == "run":
        return battery.main(["run"] + list(rest))

    if cmd == "battery":
        return battery.main(list(rest))

    if cmd == "compare":
        return compare_reports.main(list(rest))

    if cmd == "extract":
        return extract_bench.main(list(rest))

    if cmd == "seed":
        return seed_images.main(list(rest))

    if cmd == "server":
        return backend_launcher.main(list(rest))

    if cmd == "prepare":
        return prepare.main(list(rest))

    if cmd == "preflight":
        return preflight.main(list(rest))

    _print_usage()
    return 2


if __name__ == "__main__":
    sys.exit(main())
