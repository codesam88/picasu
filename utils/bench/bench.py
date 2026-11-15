#!/usr/bin/env python3
"""Unified CLI shell for the benchmark harness."""

from __future__ import annotations

import importlib.util
import json
import sys
import types
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

MODULE_FILES = {
    "cell": "run-cell.py",
    "battery": "battery.py",
    "extract": "extract-bench.py",
    "seed": "seed_images.py",
    "server": "launch-backend.py",
    "prepare": "run-prepare.py",
    "preflight": "preflight.py",
}

MODULE_ALIASES = {
    "cell": "run_cell",
    "battery": "battery",
    "extract": "extract_bench",
    "seed": "seed_images",
    "server": "launch_backend",
    "prepare": "run_prepare",
    "preflight": "preflight",
}

SUBCOMMAND_NAMES = (
    "run",
    "cell",
    "battery",
    "extract",
    "seed",
    "server",
    "prepare",
    "preflight",
    "status",
)


def _load_module(name: str, filename: str):
    if name in sys.modules:
        existing = sys.modules[name]
        if getattr(existing, "__file__", None) == str(SCRIPT_DIR / filename):
            return existing
    spec = importlib.util.spec_from_file_location(name, SCRIPT_DIR / filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


run_cell = _load_module("run_cell", "run-cell.py")
battery = _load_module("battery", "battery.py")
extract_bench = _load_module("extract_bench", "extract-bench.py")
seed_images = _load_module("seed_images", "seed_images.py")
launch_backend = _load_module("launch_backend", "launch-backend.py")
run_prepare = _load_module("run_prepare", "run-prepare.py")
preflight = _load_module("preflight", "preflight.py")

ORIGINAL_MAINS = {}


def _resolve_module(cmd: str):
    alias = MODULE_ALIASES[cmd]
    original = ORIGINAL_MAINS.get(cmd)
    candidates = []
    modules_dict = globals().get("MODULES")
    if isinstance(modules_dict, dict) and cmd in modules_dict:
        candidates.append(modules_dict[cmd])
    global_mod = globals().get(alias)
    if global_mod is not None:
        candidates.append(global_mod)
    sys_mod = sys.modules.get(alias)
    if sys_mod is not None:
        candidates.append(sys_mod)
    for candidate in candidates:
        if isinstance(candidate, types.ModuleType) and hasattr(candidate, "main"):
            main = getattr(candidate, "main")
            if original is None or main is not original:
                return candidate
    for candidate in candidates:
        if isinstance(candidate, types.ModuleType) and hasattr(candidate, "main"):
            return candidate
        if hasattr(candidate, "main"):
            return candidate
    return _load_module(alias, MODULE_FILES[cmd])


def _invoke_delegate(cmd: str, argv: list) -> int:
    module = _resolve_module(cmd)
    func = getattr(module, "main")
    script = str(SCRIPT_DIR / MODULE_FILES[cmd])
    try:
        result = func(list(argv))
    except TypeError:
        saved = sys.argv
        sys.argv = [script] + list(argv)
        try:
            result = func()
        finally:
            sys.argv = saved
    except SystemExit as exc:
        code = exc.code
        if code is None:
            return 0
        try:
            return int(code)
        except TypeError, ValueError:
            return 0 if code is None else 1
    if result is None:
        return 0
    try:
        return int(result)
    except TypeError, ValueError:
        return 0


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
        try:
            return battery.main(["run"] + list(rest))
        except SystemExit as exc:
            code = exc.code
            if code is None:
                return 0
            try:
                return int(code)
            except TypeError, ValueError:
                return 0 if code is None else 1

    handler = DISPATCH.get(cmd) if isinstance(DISPATCH, dict) else None
    if callable(handler):
        try:
            result = handler(rest)
        except SystemExit as exc:
            code = exc.code
            if code is None:
                return 0
            try:
                return int(code)
            except TypeError, ValueError:
                return 0 if code is None else 1
        if result is None:
            return 0
        try:
            return int(result)
        except TypeError, ValueError:
            return 0

    return _invoke_delegate(cmd, rest)


MODULES = {
    "cell": run_cell,
    "battery": battery,
    "extract": extract_bench,
    "seed": seed_images,
    "server": launch_backend,
    "prepare": run_prepare,
    "preflight": preflight,
}


def _make_dispatcher(cmd: str):
    def _run(argv):
        if cmd == "status":
            return _status(argv)
        return _invoke_delegate(cmd, list(argv))

    return _run


DISPATCH = {name: _make_dispatcher(name) for name in SUBCOMMAND_NAMES}
SUBCOMMANDS = DISPATCH
COMMANDS = DISPATCH
SUBCOMMAND_TABLE = DISPATCH

MAINS = {
    "cell": run_cell.main,
    "battery": battery.main,
    "extract": extract_bench.main,
    "seed": seed_images.main,
    "server": launch_backend.main,
    "prepare": run_prepare.main,
    "preflight": preflight.main,
    "status": _status,
}

for _name, _main in MAINS.items():
    if _name != "status":
        ORIGINAL_MAINS[_name] = _main

for _cmd in ("cell", "battery", "extract", "seed", "server", "prepare", "preflight"):
    ORIGINAL_MAINS[_cmd] = getattr(MODULES[_cmd], "main", None)


if __name__ == "__main__":
    sys.exit(main())
