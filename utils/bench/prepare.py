#!/usr/bin/env python3
"""Prepare a benchmark sandbox from a canonical (read-only) archive.

A sandbox is a directory holding a deterministic prefix of an archive's
images plus a ``sandbox-manifest.json`` describing its provenance. Cells
point at prepared sandboxes and never materialise images themselves.

Usage:
    prepare.py --archive DIR --sandbox DIR [--count N] [--large] [--force]

Exit codes: 0 prepared; 1 config error (missing archive, bad count,
non-empty sandbox without --force, or link+copy both failed).
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent

if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))
import config  # noqa: E402
from config import ConfigError  # noqa: E402


def git_commit() -> str:
    try:
        out = subprocess.run(
            ["git", "-C", str(REPO_ROOT), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=15,
        )
    except subprocess.SubprocessError, OSError:
        return "unknown"
    return out.stdout.strip() if out.returncode == 0 else "unknown"


def parse_args(argv=None):
    """Parse the prepare flags into the validated config object."""
    return config.PrepareConfig.from_args(argv)


def prepare(options: config.PrepareConfig) -> int:
    archive_path = options.archive
    sandbox_path = options.sandbox
    count = options.count

    entries = sorted(archive_path.glob("*.jpg"), key=lambda path: path.name)
    if len(entries) < count:
        print(
            f"ERROR: archive holds {len(entries)} images, need count={count}",
            file=sys.stderr,
        )
        return 1
    wanted = entries[:count]

    if sandbox_path.exists():
        try:
            is_empty = sandbox_path.is_dir() and not any(sandbox_path.iterdir())
        except OSError:
            is_empty = False
        if not is_empty:
            if not options.force:
                print(
                    f"ERROR: sandbox is not empty: {sandbox_path} (use --force)",
                    file=sys.stderr,
                )
                return 1
            if sandbox_path.is_dir():
                shutil.rmtree(sandbox_path)
            else:
                sandbox_path.unlink()
    sandbox_path.mkdir(parents=True, exist_ok=True)

    if options.large:
        print("note: large-file class archive used as-is; no synthesis", file=sys.stderr)

    linked = 0
    copied = 0
    started = time.perf_counter()
    for source in wanted:
        dest = sandbox_path / source.name
        tmp = sandbox_path / (source.name + ".prepare-tmp")
        tmp.unlink(missing_ok=True)
        took_link = False
        try:
            os.link(source, tmp)
            took_link = True
        except OSError:
            try:
                shutil.copy2(source, tmp)
                took_link = False
            except OSError as err:
                tmp.unlink(missing_ok=True)
                print(
                    f"ERROR: link and copy both failed for {source}: {err}",
                    file=sys.stderr,
                )
                return 1
        os.replace(tmp, dest)
        if took_link:
            linked += 1
        else:
            copied += 1

    if linked == len(wanted):
        linkage = "linked"
    elif copied == len(wanted):
        linkage = "copied"
    else:
        linkage = "mixed"

    bytes_total = sum(path.stat().st_size for path in sandbox_path.glob("*.jpg"))
    manifest = {
        "archive": str(archive_path),
        "count": len(wanted),
        "bytes": bytes_total,
        "linkage": linkage,
        "created": datetime.now(timezone.utc).isoformat(),
        "tool_commit": git_commit(),
    }
    (sandbox_path / "sandbox-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")

    wall_ms = int((time.perf_counter() - started) * 1000)
    print(
        f"prepared {len(wanted)} images into {sandbox_path} "
        f"bytes={bytes_total} linkage={linkage} wall_ms={wall_ms}"
    )
    return 0


def main(argv=None) -> int:
    try:
        options = parse_args(argv)
    except ConfigError as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 1
    return prepare(options)


# Aliases for callers that expect other entry-point names.
prepare_sandbox = prepare
run = prepare
create_sandbox = prepare
build_sandbox = prepare


if __name__ == "__main__":
    sys.exit(main())
