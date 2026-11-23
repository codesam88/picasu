#!/usr/bin/env python3
"""Host environment snapshot for benchmark runs (stdlib only).

Captured once when a battery starts (`environment.json` in the battery
parent) and refreshed when it finishes, so a later reader can tell
whether the numbers came from this machine or a differently-clocked,
differently-governed, or loaded one. Every read tolerates a missing
file: unknown fields are the string "unknown", never an exception.

Injectable `sys_root` / `proc_root` paths keep the parsers unit-testable
without touching the host.
"""

from __future__ import annotations

import os
import platform
import socket
from datetime import datetime, timezone
from pathlib import Path

UNKNOWN = "unknown"
ENVIRONMENT_FILENAME = "environment.json"


def _read_text(path: Path) -> str | None:
    try:
        return path.read_text().strip()
    except OSError:
        return None


def _read_int(text: str | None) -> int | None:
    if text is None:
        return None
    try:
        return int(text)
    except ValueError:
        return None


def _cpu_model(proc_root: Path) -> str:
    try:
        for line in (proc_root / "cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                _, _, name = line.partition(":")
                name = name.strip()
                if name:
                    return name
    except OSError:
        pass
    return UNKNOWN


def _mem_total_kb(proc_root: Path) -> int | str:
    try:
        for line in (proc_root / "meminfo").read_text().splitlines():
            if line.startswith("MemTotal:"):
                return int(line.split()[1])
    except OSError, IndexError, ValueError:
        pass
    return UNKNOWN


def _freq_mhz(sys_root: Path) -> tuple[int | str, int | str, str]:
    """(current, max, governor) across cpufreq policies, or unknowns."""
    curs, maxs, governors, drivers = [], [], set(), set()
    try:
        policies = sorted((sys_root / "devices/system/cpu/cpufreq").glob("policy*"))
    except OSError:
        policies = []
    for policy in policies:
        cur = _read_int(_read_text(policy / "scaling_cur_freq"))
        if cur is not None:
            curs.append(cur)
        top = _read_int(_read_text(policy / "scaling_max_freq"))
        if top is not None:
            maxs.append(top)
        gov = _read_text(policy / "scaling_governor")
        if gov:
            governors.add(gov)
        drv = _read_text(policy / "scaling_driver")
        if drv:
            drivers.add(drv)
    cur_mhz = round(sum(curs) / len(curs) / 1000) if curs else UNKNOWN
    max_mhz = round(max(maxs) / 1000) if maxs else UNKNOWN
    governor = ",".join(sorted(governors)) if governors else UNKNOWN
    return cur_mhz, max_mhz, governor


def _no_turbo(sys_root: Path) -> int | str:
    """1 when turbo is disabled, 0 when enabled, unknown when not exposed."""
    raw = _read_text(sys_root / "devices/system/cpu/intel_pstate/no_turbo")
    if raw is None:
        return UNKNOWN
    try:
        return int(raw)
    except ValueError:
        return UNKNOWN


def _git_identity(repo: Path | None) -> tuple[str, str, bool]:
    """(branch, commit, dirty) for the repo checkout, or unknowns."""
    import subprocess

    def run(*args: str) -> str | None:
        try:
            result = subprocess.run(
                ["git", *args],
                cwd=str(repo) if repo is not None else None,
                capture_output=True,
                text=True,
                timeout=10,
            )
        except OSError:
            return None
        if result.returncode != 0:
            return None
        return result.stdout.strip() or None

    branch = run("rev-parse", "--abbrev-ref", "HEAD") or UNKNOWN
    commit = run("rev-parse", "HEAD") or UNKNOWN
    status = run("status", "--porcelain")
    dirty = bool(status) if status is not None else UNKNOWN
    return branch, commit, dirty


def collect_environment(
    *,
    images: int | None = None,
    sys_root: str | Path = "/sys",
    proc_root: str | Path = "/proc",
    git_root: str | Path | None = None,
    now: datetime | None = None,
) -> dict:
    """Snapshot the host environment for one battery run."""
    sys_root = Path(sys_root)
    proc_root = Path(proc_root)
    if git_root is None:
        git_root = Path(__file__).resolve().parent.parent.parent
    try:
        load1, load5, load15 = os.getloadavg()
    except OSError:
        load1 = load5 = load15 = UNKNOWN
    cpu_count = os.cpu_count() or UNKNOWN
    cur_mhz, max_mhz, governor = _freq_mhz(sys_root)
    branch, commit, dirty = _git_identity(Path(git_root))
    moment = now or datetime.now(timezone.utc)
    return {
        "images": images,
        "started_utc": moment.isoformat(timespec="seconds"),
        "hostname": socket.gethostname(),
        "kernel": platform.release(),
        "cpu_model": _cpu_model(proc_root),
        "cpu_count": cpu_count,
        "freq_cur_mhz": cur_mhz,
        "freq_max_mhz": max_mhz,
        "governor": governor,
        "turbo_disabled": _no_turbo(sys_root),
        "mem_total_kb": _mem_total_kb(proc_root),
        "load_1": load1,
        "load_5": load5,
        "load_15": load15,
        "git_branch": branch,
        "git_commit": commit,
        "git_dirty": dirty,
        "ended_utc": None,
        "end_load_1": None,
        "end_load_5": None,
        "end_load_15": None,
    }


def finish_environment(record: dict, *, now: datetime | None = None) -> dict:
    """Stamp completion time and load onto a collected record."""
    moment = now or datetime.now(timezone.utc)
    try:
        load1, load5, load15 = os.getloadavg()
    except OSError:
        load1 = load5 = load15 = UNKNOWN
    record = dict(record)
    record["ended_utc"] = moment.isoformat(timespec="seconds")
    record["end_load_1"] = load1
    record["end_load_5"] = load5
    record["end_load_15"] = load15
    return record


def load_environment(path: str | Path) -> dict | None:
    """Read a stored environment record; None when absent or unparsable."""
    import json

    try:
        data = json.loads(Path(path).read_text())
    except OSError, ValueError:
        return None
    return data if isinstance(data, dict) else None


def write_environment(path: str | Path, record: dict) -> None:
    """Write a record as JSON; creates parent dirs."""
    import json

    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
