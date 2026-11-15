#!/usr/bin/env python3
"""Preflight checklist: refuse a cell whose preconditions do not hold.

One checklist phase, one message shape. Each check is a `Check` value
(`name`, `ok`, `detail`, optional `hint`); `main()` prints one line per
check — `ok <name>: <detail>` / `FAIL <name>: <detail+hint>` — and exits
1 when any check fails. No benchmark process is started and nothing is
mutated: the OUT check reports writability instead of probing with a
write, and a busy port only triggers the read-only `ss` owner lookup.

Usage:
    preflight.py [--out DIR] [--fresh 0|1] [--port N]
                 [--archive DIR] [--sandbox DIR] [--binary PATH]

Flags override the environment; unset inputs fall back to the launcher
contract (`PICASU_BIN`/`BUILD_PROFILE`, `HYPERFINE_BIN`, `PORT`, `OUT`,
`FRESH`, `IMAGE_HOME_SRC`). The archive check runs only when `--archive`
names the canonical archive a `bench prepare` will consume, and the
sandbox check only when a prepared sandbox is in play.

The port, OUT and sandbox rules are the launcher's: this module calls
`backend_launcher` instead of restating them, so the standalone checklist
and the guard inside a running cell cannot drift apart.

Exit codes: 0 all checks pass; 1 at least one check failed.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent

# The checks call into the launcher for the port, OUT, binary and sandbox
# rules; keep a single importable module instance across callers. The
# checklist inputs themselves are normalised by config.PreflightConfig.
sys.path.insert(0, str(SCRIPT_DIR))
import backend_launcher  # noqa: E402
import config  # noqa: E402

HYPERFINE_HINT = "run: just install hyperfine"
SANDBOX_MANIFEST = "sandbox-manifest.json"


@dataclass(frozen=True)
class Check:
    """One precondition and its verdict."""

    name: str
    ok: bool
    detail: str
    hint: str = ""

    def message(self) -> str:
        """Detail with the remedy appended, for callers that raise."""
        return f"{self.detail} ({self.hint})" if self.hint else self.detail

    def line(self) -> str:
        prefix = "ok" if self.ok else "FAIL"
        return f"{prefix} {self.name}: {self.message()}"


# --- individual checks --------------------------------------------------------
def check_server_binary(environ: dict, binary=None) -> Check:
    """Server binary exists and is executable."""
    if binary is None:
        try:
            binary, _explicit, profile = backend_launcher.resolve_binary(environ)
        except backend_launcher.ConfigError as err:
            return Check("binary", False, str(err))
    else:
        binary = Path(binary)
        profile = environ.get("BUILD_PROFILE", "release")
    if not backend_launcher.executable_file(binary):
        return Check(
            "binary",
            False,
            f"missing executable server binary {binary}",
            hint=f"build it: cargo build --profile {profile} --bin picasu",
        )
    return Check("binary", True, str(binary))


def check_hyperfine(environ: dict) -> Check:
    """The hyperfine binary the workload will drive."""
    explicit = environ.get("HYPERFINE_BIN")
    if explicit is not None:
        if explicit == "":
            return Check("hyperfine", False, "hyperfine is required", HYPERFINE_HINT)
        if not backend_launcher.executable_file(Path(explicit)):
            return Check(
                "hyperfine",
                False,
                f"HYPERFINE_BIN is not executable: {explicit}",
                HYPERFINE_HINT,
            )
        return Check("hyperfine", True, explicit)
    found = shutil.which("hyperfine")
    if found is None:
        return Check("hyperfine", False, "hyperfine is required", HYPERFINE_HINT)
    return Check("hyperfine", True, found)


def check_archive(archive) -> Check:
    """Canonical archive holds images for `bench prepare` to link."""
    path = Path(archive)
    if not path.is_dir():
        return Check(
            "archive",
            False,
            f"archive directory not found: {path}",
            hint="seed one with `just bench seed-images`",
        )
    try:
        images = [entry for entry in path.iterdir() if entry.is_file() and entry.suffix == ".jpg"]
    except OSError as err:
        return Check("archive", False, f"cannot read archive {path}: {err}")
    if not images:
        return Check(
            "archive",
            False,
            f"archive holds no .jpg images: {path}",
            hint="seed one with `just bench seed-images`",
        )
    return Check("archive", True, f"{path} ({len(images)} jpg)")


def check_sandbox_manifest(sandbox) -> Check:
    """Sandbox carries the manifest `bench prepare` writes."""
    try:
        sandbox_path = backend_launcher.validate_image_home_src(str(sandbox))
    except backend_launcher.ConfigError as err:
        return Check("sandbox", False, str(err))
    detail = str(sandbox_path)
    try:
        manifest = json.loads((sandbox_path / SANDBOX_MANIFEST).read_text())
    except OSError, json.JSONDecodeError:
        manifest = {}
    count = manifest.get("count") if isinstance(manifest, dict) else None
    if isinstance(count, int):
        detail = f"{detail} (count={count})"
    return Check("sandbox", True, detail)


def check_port(port) -> Check:
    """Nothing is listening on the cell's server port."""
    try:
        port_number = backend_launcher.validate_port(port)
    except backend_launcher.ConfigError as err:
        return Check("port", False, str(err))
    if backend_launcher.port_in_use(port_number):
        holder = " ".join(backend_launcher.port_holder(port_number).split())
        return Check(
            "port",
            False,
            f"port {port_number} already in use: {holder or '(no owner detail)'}",
            hint="pick a free PORT or stop the holder",
        )
    return Check("port", True, f"{port_number} free")


def check_out(out, fresh: bool) -> Check:
    """OUT is safe to wipe (fresh) and writable."""
    try:
        out_path = backend_launcher.validate_out(str(out), fresh=fresh)
    except backend_launcher.ConfigError as err:
        return Check("out", False, str(err))
    if out_path.exists():
        if not out_path.is_dir():
            return Check("out", False, f"OUT exists but is not a directory: {out_path}")
        if not os.access(out_path, os.W_OK | os.X_OK):
            return Check("out", False, f"OUT is not writable: {out_path}")
        return Check("out", True, f"{out_path} writable")
    parent = _nearest_existing(out_path.parent)
    if parent is None or not os.access(parent, os.W_OK | os.X_OK):
        return Check("out", False, f"OUT parent is not writable: {parent or out_path.parent}")
    return Check("out", True, f"{out_path} will be created")


def _nearest_existing(path: Path) -> Path | None:
    current = path
    while True:
        if current.exists():
            return current
        if current.parent == current:
            return None
        current = current.parent


# --- checklist ----------------------------------------------------------------
def preflight(
    environ: dict,
    *,
    out=None,
    fresh: bool | None = None,
    port=None,
    archive=None,
    sandbox=None,
    binary=None,
    require_hyperfine: bool = True,
) -> list[Check]:
    """Build the checklist for one cell (or one sandbox preparation).

    `binary`, `out`, `fresh` and `port` are caller context: a caller that
    already resolved a value passes it in rather than letting the check
    re-derive a different one. `require_hyperfine=False` is the
    server-only subset (no workload will run).
    """
    cfg = config.PreflightConfig.from_env(
        environ,
        out=out,
        fresh=fresh,
        port=port,
        archive=archive,
        sandbox=sandbox,
        binary=binary,
        require_hyperfine=require_hyperfine,
    )
    checks = [check_server_binary(cfg.environ, binary=cfg.binary)]
    if cfg.require_hyperfine:
        checks.append(check_hyperfine(cfg.environ))
    if cfg.archive is not None:
        checks.append(check_archive(cfg.archive))
    if cfg.sandbox:
        checks.append(check_sandbox_manifest(cfg.sandbox))
    if cfg.out:
        checks.append(check_out(cfg.out, fresh=cfg.fresh))
    checks.append(check_port(cfg.port))
    return checks


def print_checklist(checks: list[Check]) -> list[Check]:
    """Print every check (ok on stdout, FAIL on stderr); return the failures."""
    failed = []
    for check in checks:
        print(check.line(), file=sys.stdout if check.ok else sys.stderr, flush=True)
        if not check.ok:
            failed.append(check)
    return failed


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", help="cell artifact directory (default: $OUT)")
    parser.add_argument(
        "--fresh",
        choices=("0", "1"),
        help="FRESH semantics for the OUT check (default: $FRESH or 1)",
    )
    parser.add_argument("--port", help="server port (default: $PORT or 5713)")
    parser.add_argument("--archive", help="canonical archive `bench prepare` will consume")
    parser.add_argument("--sandbox", help="prepared sandbox (default: $IMAGE_HOME_SRC)")
    parser.add_argument(
        "--binary",
        help="server binary (default: $PICASU_BIN or target/<profile>/picasu)",
    )
    return parser.parse_args(argv)


def main(argv=None) -> int:
    args = parse_args(argv)
    environ = dict(os.environ)
    checks = preflight(
        environ,
        out=args.out,
        fresh=None if args.fresh is None else args.fresh == "1",
        port=args.port,
        archive=args.archive,
        sandbox=args.sandbox,
        binary=args.binary,
    )
    return 1 if print_checklist(checks) else 0


if __name__ == "__main__":
    sys.exit(main())
