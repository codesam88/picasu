#!/usr/bin/env python3
"""Orchestrate one benchmark cell: launcher + workload + shutdown.

`launch-backend.py` owns the server half and `run-workload.py` the measurement
half. This module runs them in the order a cell requires and guarantees the
server is stopped even when the workload fails.

Sequence:

  1. start `launch-backend.py` in its own process group with the cell env;
  2. wait (bounded) for `OUT/launcher-info.json` and a live
     `/get/server-info` probe (default 150 s);
  3. run `run-workload.py` with `LAUNCHER_INFO`, `OUT`, and the rest of the
     environment unchanged;
  4. on a workload failure or timeout, freeze diagnostics into
     `OUT/diag-orchestrator/` while the server is still alive: server tail,
     thread count, per-thread `/proc` wchan/stack/syscall (ptrace-free),
     best-effort gdb stacks (system gdb first, then rust-gdb, then PATH gdb),
     and the latest status snapshot. A status
     file is only named `status-final.json` when the server reported a
     terminal state; a running snapshot stays `status-last.json`;
  5. SIGTERM the launcher's process group and require its documented
     interrupt exit code (130).

Environment contract:

  OUT                       required (or --out); cell artifact directory
  PORT                      required (or --port); server port
  BUILD_PROFILE             release (default) | debug; release without
                            RUST_LOG counts as canonical upstream
  LAUNCHER_READY_TIMEOUT_S  budget for launcher-info.json + readiness
                            (default 150)
  LAUNCHER_STOP_TIMEOUT_S   SIGTERM grace before SIGKILL (default 30)
  WORKLOAD_TIMEOUT_S        orchestrator budget for the workload; on expiry
                            the workload is killed and diagnostics frozen
                            (default 0, disabled)
  GDB                       0 disables stack capture (default 1)

All other environment variables pass through to both children unchanged.

Exit codes: 0 workload completed; 1 config/preflight; 10 launcher never
became ready; 11 workload could not be spawned; 12 launcher did not stop
gracefully; 13 workload timed out; 130 orchestrator interrupted; otherwise
the workload exit code.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent

# The launcher owns process supervision and stack capture; reuse those
# primitives here instead of reimplementing them.
sys.path.insert(0, str(SCRIPT_DIR))
import backend_launcher  # noqa: E402

DEFAULT_READY_TIMEOUT_S = 150.0
DEFAULT_STOP_TIMEOUT_S = 30.0
POLL_INTERVAL_S = 0.2
PROBE_TIMEOUT_S = 2.0
SERVER_INFO_PATH = "/get/server-info"
LAUNCHER_INTERRUPT_RC = 130
TERMINAL_STATUS_STATES = ("completed", "failed", "canceled")

EXIT_CONFIG = 1
EXIT_LAUNCHER_NOT_READY = 10
EXIT_WORKLOAD_SPAWN = 11
EXIT_SHUTDOWN_FAILED = 12
EXIT_WORKLOAD_TIMEOUT = 13
EXIT_INTERRUPTED = 130

LAUNCHER_SCRIPT = SCRIPT_DIR / "launch-backend.py"
WORKLOAD_SCRIPT = SCRIPT_DIR / "run-workload.py"


class ConfigError(Exception):
    """Invalid orchestrator configuration."""


def log_line(message: str) -> None:
    print(f"{time.strftime('%H:%M:%S')} {message}", flush=True)


# --- config ------------------------------------------------------------------
def _validate_out(raw: str) -> Path:
    if raw == "":
        raise ConfigError("OUT is required (environment OUT or --out)")
    out = Path(os.path.realpath(raw))
    if str(out) == "/":
        raise ConfigError("refusing to use OUT='/'")
    if out == REPO_ROOT or REPO_ROOT in out.parents:
        raise ConfigError(f"OUT must not be inside the repo: {out}")
    return out


def _port(raw: str) -> int:
    try:
        port = int(raw)
    except ValueError:
        raise ConfigError(f"PORT must be an integer, got '{raw}'")
    if not 1 <= port <= 65535:
        raise ConfigError(f"PORT must be in 1..65535, got {port}")
    return port


def _positive_float(environ: dict, name: str, default: float) -> float:
    raw = environ.get(name, str(default))
    try:
        value = float(raw)
    except ValueError:
        raise ConfigError(f"{name} must be a number, got '{raw}'")
    if value <= 0:
        raise ConfigError(f"{name} must be > 0, got {value}")
    return value


def _nonnegative_float(environ: dict, name: str, default: float) -> float:
    raw = environ.get(name, str(default))
    try:
        value = float(raw)
    except ValueError:
        raise ConfigError(f"{name} must be a number, got '{raw}'")
    if value < 0:
        raise ConfigError(f"{name} must be >= 0, got {value}")
    return value


def config_from_env(environ: dict, out_flag=None, port_flag=None) -> dict:
    """Build cell config. Flags win over environment; mode is derived downstream."""
    out = _validate_out(out_flag if out_flag is not None else environ.get("OUT", ""))
    raw_port = port_flag if port_flag is not None else environ.get("PORT", "")
    if raw_port == "":
        raise ConfigError("PORT is required (environment PORT or --port)")
    port = _port(str(raw_port))

    build_profile = environ.get("BUILD_PROFILE", "release")
    if build_profile not in ("release", "debug"):
        raise ConfigError(f"BUILD_PROFILE must be release or debug, got '{build_profile}'")

    return {
        "out": out,
        "port": port,
        "build_profile": build_profile,
        "ready_timeout": _positive_float(
            environ, "LAUNCHER_READY_TIMEOUT_S", DEFAULT_READY_TIMEOUT_S
        ),
        "stop_timeout": _positive_float(environ, "LAUNCHER_STOP_TIMEOUT_S", DEFAULT_STOP_TIMEOUT_S),
        "workload_timeout": _nonnegative_float(environ, "WORKLOAD_TIMEOUT_S", 0.0),
        "gdb": environ.get("GDB", "1") != "0",
        "poll_interval": POLL_INTERVAL_S,
        "launcher_script": LAUNCHER_SCRIPT,
        "workload_script": WORKLOAD_SCRIPT,
    }


# --- child process control ---------------------------------------------------
def _signal_group(proc, sig) -> None:
    try:
        os.killpg(os.getpgid(proc.pid), sig)
    except ProcessLookupError, PermissionError, OSError:
        try:
            proc.send_signal(sig)
        except ProcessLookupError, OSError:
            pass


def spawn_launcher(cfg: dict, environ: dict, output_path: Path):
    """Start the launcher CLI in its own process group.

    Launcher output goes to a side file because the launcher may wipe OUT
    before it creates its own launcher.log.
    """
    env = dict(environ)
    env.update(
        {
            "OUT": str(cfg["out"]),
            "PORT": str(cfg["port"]),
            "BUILD_PROFILE": cfg["build_profile"],
        }
    )
    command = [sys.executable, str(cfg["launcher_script"])]
    log_line(f"launcher start: port={cfg['port']} out={cfg['out']} profile={cfg['build_profile']}")
    handle = open(output_path, "w", encoding="utf-8")  # noqa: PTH123
    try:
        return subprocess.Popen(
            command,
            env=env,
            stdout=handle,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
    finally:
        handle.close()


def stop_launcher(proc, grace: float) -> int:
    """SIGTERM the launcher group, escalating to SIGKILL after `grace`."""
    if proc.poll() is not None:
        return proc.returncode
    log_line("launcher shutdown: SIGTERM")
    _signal_group(proc, signal.SIGTERM)
    try:
        return proc.wait(timeout=grace)
    except subprocess.TimeoutExpired:
        log_line(f"launcher did not stop within {grace:g}s; SIGKILL")
        _signal_group(proc, signal.SIGKILL)
        try:
            return proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            return proc.poll() if proc.poll() is not None else -1


# --- readiness ---------------------------------------------------------------
def probe_ready(base_url: str, timeout: float = PROBE_TIMEOUT_S) -> bool:
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(base_url.rstrip("/") + SERVER_INFO_PATH, timeout=timeout) as resp:
            return resp.status == 200
    except urllib.error.HTTPError as err:
        err.close()
        return False
    except urllib.error.URLError, OSError:
        return False


def _read_launcher_info(path: Path):
    try:
        info = json.loads(path.read_text())
    except OSError, json.JSONDecodeError:
        return None
    return info if isinstance(info, dict) and info.get("base_url") else None


def wait_ready(cfg: dict, proc) -> tuple:
    """Return ('ready', info) | ('exited', rc) | ('timeout', None)."""
    info_path = cfg["out"] / "launcher-info.json"
    deadline = time.monotonic() + cfg["ready_timeout"]
    while time.monotonic() < deadline:
        rc = proc.poll()
        if rc is not None:
            return "exited", rc
        info = _read_launcher_info(info_path)
        if info is not None and probe_ready(str(info["base_url"])):
            return "ready", info
        time.sleep(cfg["poll_interval"])
    return "timeout", None


# --- workload ----------------------------------------------------------------
def run_workload(cfg: dict, environ: dict, info: dict) -> int:
    env = dict(environ)
    env["LAUNCHER_INFO"] = str(cfg["out"] / "launcher-info.json")
    env["OUT"] = str(cfg["out"])
    env["BUILD_PROFILE"] = cfg["build_profile"]
    command = [sys.executable, str(cfg["workload_script"])]
    log_line(f"workload start: {info.get('base_url')} out={cfg['out']}")
    timeout = cfg.get("workload_timeout") or None
    completed = subprocess.run(command, env=env, timeout=timeout)
    log_line(f"workload exited rc={completed.returncode}")
    return completed.returncode


# --- diagnostics -------------------------------------------------------------
def _tail(path: Path, lines: int) -> str:
    try:
        return "\n".join(path.read_text(errors="replace").splitlines()[-lines:])
    except OSError:
        return ""


def _copy_if_file(src: Path, dest: Path) -> None:
    if not src.is_file():
        return
    try:
        shutil.copyfile(src, dest)
    except OSError as err:
        log_line(f"diagnostics: could not copy {src}: {err}")


def _read_json(path: Path):
    try:
        return json.loads(path.read_text())
    except OSError, json.JSONDecodeError:
        return None


def _server_pid(cfg: dict):
    info = _read_json(cfg["out"] / "launcher-info.json")
    pid = info.get("pid") if isinstance(info, dict) else None
    return pid if isinstance(pid, int) and pid > 0 else None


def _status_state(cfg: dict) -> str:
    doc = _read_json(cfg["out"] / "status-last.json")
    state = doc.get("state") if isinstance(doc, dict) else None
    return state if isinstance(state, str) else "unknown"


def _read_proc_file(path: Path) -> tuple:
    """Return `(text, error)` for one `/proc` file.

    Unreadable files are normal: `stack` and `syscall` need ptrace
    privileges on this kernel, so callers record the failure rather than
    treating it as a capture bug.
    """
    try:
        return path.read_text(errors="replace").strip(), None
    except OSError as err:
        return "", str(err)


def capture_proc_thread_state(pid: int, diag: Path) -> dict:
    """Freeze per-thread `/proc` state while the server is still alive.

    Writes `threads.txt` (count) and `proc-threads.txt` (one block per tid:
    `comm`, `wchan`, `stack`, `syscall`). Returns a summary whose per-field
    read counts show which probes the kernel actually permitted.
    """
    task_dir = Path(f"/proc/{pid}/task")
    try:
        tids = sorted(entry.name for entry in task_dir.iterdir() if entry.name.isdigit())
    except OSError as err:
        log_line(f"diagnostics: /proc task listing failed: {err}")
        return {"ok": False, "error": str(err)}

    fields = ("wchan", "stack", "syscall")
    readable = {field: 0 for field in fields}
    blocks = []
    for tid in tids:
        base = task_dir / tid
        comm, _ = _read_proc_file(base / "comm")
        blocks.append(f"tid={tid} comm={comm or '?'}")
        for field in fields:
            text, err = _read_proc_file(base / field)
            if err is not None:
                blocks.append(f"{field}=!{err}")
                continue
            readable[field] += 1
            flat = text.replace("\n", " | ")
            blocks.append(f"{field}={flat}")
        blocks.append("")

    try:
        (diag / "proc-threads.txt").write_text("\n".join(blocks))
        (diag / "threads.txt").write_text(f"{len(tids)}\n")
    except OSError as err:
        log_line(f"diagnostics: could not write /proc thread state: {err}")
        return {"ok": False, "threads": len(tids), "error": str(err), **readable}
    log_line(f"diagnostics: /proc state for {len(tids)} threads -> {diag / 'proc-threads.txt'}")
    return {"ok": True, "threads": len(tids), **readable}


def capture_live_stacks(cfg: dict, diag: Path) -> dict:
    """Capture `/proc` thread state plus best-effort gdb stacks.

    The `/proc` capture is the ptrace-independent fallback and always runs
    while the server lives; `backend_launcher.capture_stacks` adds
    `threads.txt` and, when gdb can attach, `stacks.txt`.
    """
    pid = _server_pid(cfg)
    captures = {
        "proc_threads": {"ok": False, "reason": "no live server pid"},
        "gdb": {"ok": False, "reason": "no live server pid"},
    }
    if pid is None:
        log_line("diagnostics: no live server pid; skipping stack capture")
        return captures

    logf = None
    try:
        logf = open(diag / "stack-capture.log", "a", encoding="utf-8")  # noqa: PTH123
        captures["proc_threads"] = capture_proc_thread_state(pid, diag)
        gdb_result = backend_launcher.capture_stacks(pid, diag, logf, cfg.get("gdb", True))
        stacks = diag / "stacks.txt"
        captures["gdb"] = {
            **gdb_result,
            "file": str(stacks) if stacks.is_file() else None,
        }
    except OSError as err:
        log_line(f"diagnostics: stack capture setup failed: {err}")
        captures["gdb"] = {"ok": False, "error": str(err)}
    finally:
        if logf is not None:
            logf.close()
    return captures


def collect_diag(
    cfg: dict,
    reason: str,
    *,
    workload_rc=None,
    launcher_rc=None,
    launcher_output: Path | None = None,
    capture_live: bool = False,
) -> None:
    """Freeze server tail, status snapshot, and launcher logs inside OUT.

    With `capture_live` the server is still running, so thread counts and
    gdb stacks are taken before the caller's shutdown path runs.
    """
    out = cfg["out"]
    diag = out / "diag-orchestrator"
    try:
        diag.mkdir(parents=True, exist_ok=True)
    except OSError as err:
        log_line(f"diagnostics: could not create {diag}: {err}")
        return

    state = _status_state(cfg)
    captures = capture_live_stacks(cfg, diag) if capture_live else None
    meta = {
        "reason": reason,
        "out": str(out),
        "port": cfg["port"],
        "build_profile": cfg["build_profile"],
        "workload_rc": workload_rc,
        "launcher_rc": launcher_rc,
        "status_state": state,
        "server_pid": _server_pid(cfg),
        "captured": time.strftime("%Y-%m-%dT%H:%M:%S"),
    }
    if captures is not None:
        meta["captures"] = captures
    try:
        (diag / "orchestrator.json").write_text(json.dumps(meta, indent=2) + "\n")
    except OSError as err:
        log_line(f"diagnostics: could not write metadata: {err}")

    tail = _tail(out / "server.log", 200)
    if tail:
        try:
            (diag / "server-tail.log").write_text(tail + "\n")
        except OSError as err:
            log_line(f"diagnostics: could not write server tail: {err}")

    # `status-last.json` is always the newest snapshot. `status-final.json`
    # only earns its name when the state is terminal; a running state must
    # not be frozen under a terminal name.
    _copy_if_file(out / "status-last.json", diag / "status-last.json")
    if state in TERMINAL_STATUS_STATES:
        _copy_if_file(out / "status-final.json", diag / "status-final.json")
    else:
        (diag / "status-final.json").unlink(missing_ok=True)

    for name in ("launcher-info.json", "launcher.log"):
        _copy_if_file(out / name, diag / name)
    if launcher_output is not None:
        _copy_if_file(launcher_output, diag / "launcher-output.log")

    log_line(f"diagnostics: {diag} ({reason})")


# --- cell --------------------------------------------------------------------
def _temp_log() -> Path:
    fd, path = tempfile.mkstemp(prefix="run-cell-launcher-", suffix=".log")
    os.close(fd)
    return Path(path)


def run_cell(cfg: dict, environ: dict) -> int:
    """Run one cell. Returns the orchestrator exit code."""
    launcher_output = _temp_log()
    launcher = None
    shutdown_rc = None
    result = None
    try:
        launcher = spawn_launcher(cfg, environ, launcher_output)
        state, payload = wait_ready(cfg, launcher)
        if state == "ready":
            info = payload
            log_line(f"launcher ready: {info['base_url']}")
            try:
                workload_rc = run_workload(cfg, environ, info)
            except subprocess.TimeoutExpired:
                log_line(f"workload exceeded {cfg['workload_timeout']:g}s; killing it")
                collect_diag(
                    cfg,
                    "workload-timeout",
                    workload_rc=EXIT_WORKLOAD_TIMEOUT,
                    launcher_output=launcher_output,
                    capture_live=True,
                )
                result = EXIT_WORKLOAD_TIMEOUT
            except OSError as err:
                log_line(f"workload spawn failed: {err}")
                collect_diag(
                    cfg,
                    "workload-spawn-failed",
                    workload_rc=EXIT_WORKLOAD_SPAWN,
                    launcher_output=launcher_output,
                    capture_live=True,
                )
                result = EXIT_WORKLOAD_SPAWN
            else:
                if workload_rc != 0:
                    collect_diag(
                        cfg,
                        "workload-failed",
                        workload_rc=workload_rc,
                        launcher_output=launcher_output,
                        capture_live=True,
                    )
                result = workload_rc
        elif state == "exited":
            log_line(f"launcher exited before ready (rc={payload})")
            collect_diag(
                cfg,
                "launcher-exited",
                launcher_rc=payload,
                launcher_output=launcher_output,
            )
            result = EXIT_LAUNCHER_NOT_READY
        else:
            log_line(f"launcher not ready within {cfg['ready_timeout']:g}s")
            collect_diag(
                cfg,
                "launcher-not-ready",
                launcher_output=launcher_output,
            )
            result = EXIT_LAUNCHER_NOT_READY
    except KeyboardInterrupt:
        log_line("interrupted")
        result = EXIT_INTERRUPTED
    finally:
        if launcher is not None:
            shutdown_rc = stop_launcher(launcher, cfg["stop_timeout"])
            log_line(f"launcher stopped rc={shutdown_rc}")
        launcher_output.unlink(missing_ok=True)

    if result == EXIT_LAUNCHER_NOT_READY:
        return EXIT_LAUNCHER_NOT_READY
    if result == EXIT_INTERRUPTED:
        return EXIT_INTERRUPTED
    if shutdown_rc != LAUNCHER_INTERRUPT_RC:
        log_line(
            f"launcher shutdown was not graceful (expected rc={LAUNCHER_INTERRUPT_RC}, "
            f"got {shutdown_rc})"
        )
        return EXIT_SHUTDOWN_FAILED
    return result


def _interrupt(signum, frame):
    raise KeyboardInterrupt


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", help="cell artifact directory (default: $OUT)")
    parser.add_argument("--port", help="server port (default: $PORT)")
    args = parser.parse_args(argv)

    # SIGTERM unwinds through run_cell's finally so the launcher and its
    # server are stopped instead of orphaned.
    signal.signal(signal.SIGTERM, _interrupt)
    environ = dict(os.environ)
    try:
        cfg = config_from_env(environ, args.out, args.port)
    except ConfigError as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return EXIT_CONFIG
    return run_cell(cfg, environ)


if __name__ == "__main__":
    sys.exit(main())
