#!/usr/bin/env python3
"""Seed a reusable benchmark image repo via snapfab batch (stdlib only).

Images are generated a priori; bench cells only copy from a repo, never
generate. Entry naming is zero-padded so sorted order is generation order
and any cell size takes a deterministic prefix.

Usage:
    seed_images.py --repo DIR --count N [--template FILE] [--jobs J]
                   [--snapfab BIN] [--profile release|debug]

Without --template, entries are synthesized (jpeg 4000x3000, spread
exif dates, bench/seed tags). With --template, the first N entries of
the PhotoSpec list are used; the template must hold at least N.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent


DEFAULT_WIDTH = 4000
DEFAULT_HEIGHT = 3000
LARGE_WIDTH = 6000
LARGE_HEIGHT = 4000
DEFAULT_LARGE_BYTES = 5_000_000


def synth_entries(count: int, large: bool = False) -> list:
    width = LARGE_WIDTH if large else DEFAULT_WIDTH
    height = LARGE_HEIGHT if large else DEFAULT_HEIGHT
    entries = []
    for i in range(1, count + 1):
        day = (i % 28) + 1
        hour = (i - 1) % 24
        entry = {
            "output": f"img_{i:05d}.jpg",
            "format": "jpeg",
            "width": width,
            "height": height,
            "exif_date": f"2024:06:{day:02d} {hour:02d}:15:00",
            "tags": ["bench", "seed"],
        }
        entries.append(entry)
    return entries


def load_template(path: Path, count: int) -> list:
    try:
        entries = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as err:
        raise SystemExit(f"error: cannot read template {path}: {err}")
    if not isinstance(entries, list) or len(entries) < count:
        raise SystemExit(
            f"error: template {path} holds "
            f"{len(entries) if isinstance(entries, list) else 0} entries, need {count}"
        )
    return entries[:count]


def run_batch(
    snapfab: Path, repo: Path, entries: list, jobs: int, logf, generators: str = ""
) -> None:
    jobs = max(1, min(jobs, len(entries)))
    chunk_dir = repo / "manifest-chunks"
    if chunk_dir.exists():
        shutil.rmtree(chunk_dir)
    chunk_dir.mkdir(parents=True)
    processes = []
    for shard in range(jobs):
        chunk = [entry for index, entry in enumerate(entries) if index % jobs == shard]
        chunk_path = chunk_dir / f"chunk-{shard}.json"
        chunk_path.write_text(json.dumps(chunk))
        log_path = repo / f"gen-chunk-{shard}.log"
        handle = open(log_path, "w", encoding="utf-8")  # noqa: PTH123
        logf(f"seed: shard {shard + 1}/{jobs} ({len(chunk)} images)")
        command = [str(snapfab), "batch", str(chunk_path)]
        if generators:
            command = [str(snapfab), "batch", str(chunk_path), "-g", generators]
        processes.append(
            (
                handle,
                subprocess.Popen(
                    command,
                    cwd=str(repo),
                    stdout=handle,
                    stderr=subprocess.STDOUT,
                ),
            )
        )
    failed = False
    for handle, process in processes:
        if process.wait() != 0:
            failed = True
        handle.close()
    if failed:
        raise SystemExit("error: snapfab batch failed (see gen-chunk-*.log in repo)")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, help="seeded image directory")
    parser.add_argument("--count", required=True, type=int, help="images to generate")
    parser.add_argument("--template", default="", help="PhotoSpec JSON list (optional)")
    parser.add_argument(
        "--jobs", default=0, type=int, help="parallel workers (default min(4,nproc))"
    )
    parser.add_argument(
        "--snapfab", default="", help="snapfab binary (default target/<profile>/snapfab)"
    )
    parser.add_argument(
        "--profile", default="release", help="build profile for the default binary path"
    )
    parser.add_argument("--large", action="store_true", help="synthesize large (~5MB) images")
    parser.add_argument("--bytes", type=int, default=0, help="pad each image to at least N bytes")
    args = parser.parse_args(argv)

    if args.count < 1:
        raise SystemExit("error: --count must be >= 1")
    target_bytes = args.bytes
    if args.large and target_bytes == 0:
        target_bytes = DEFAULT_LARGE_BYTES
    repo = Path(args.repo)
    repo.mkdir(parents=True, exist_ok=True)
    stale = sorted(repo.glob("*.jpg"))
    if stale and os.environ.get("OVERWRITE", "0") != "1":
        raise SystemExit(
            f"error: repo {repo} already holds {len(stale)} images "
            "(refusing to mix; set OVERWRITE=1 to reseed)"
        )
    for path in stale:
        path.unlink()

    entries = (
        load_template(Path(args.template), args.count)
        if args.template
        else synth_entries(args.count, large=args.large)
    )
    (repo / "manifest.json").write_text(
        "[" + ",".join(json.dumps(e, separators=(",", ":")) for e in entries) + "]\n"
    )

    if args.profile not in ("release", "debug"):
        raise SystemExit(f"error: --profile must be release|debug, got {args.profile}")
    snapfab = (
        Path(args.snapfab) if args.snapfab else REPO_ROOT / "target" / args.profile / "snapfab"
    )
    if not (snapfab.is_file() and os.access(snapfab, os.X_OK)):
        raise SystemExit(
            f"error: missing executable snapfab binary {snapfab} "
            f"(build it: cargo build --profile {args.profile} -p snapfab)"
        )

    jobs = args.jobs or min(4, os.cpu_count() or 1)
    start = time.perf_counter()
    generators = "julia" if args.large else ""
    run_batch(snapfab, repo, entries, jobs, print, generators=generators)
    jpgs = sorted(repo.glob("*.jpg"))
    if len(jpgs) != args.count:
        raise SystemExit(f"error: expected {args.count} images, found {len(jpgs)}")
    if target_bytes:
        for path in jpgs:
            size = path.stat().st_size
            if size < target_bytes:
                with open(path, "ab") as handle:  # noqa: PTH123
                    handle.write(b"\x00" * (target_bytes - size))
    jpgs = sorted(repo.glob("*.jpg"))
    wall_s = time.perf_counter() - start
    total = sum(path.stat().st_size for path in jpgs)
    print(f"seeded {len(jpgs)} images ({total} bytes) in {wall_s:.1f}s -> {repo}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
