#!/usr/bin/env python3
"""Throwaway backend launcher for benchmark and debug runs.

Owns the server-management half of a benchmark cell: state setup, port
preflight, spawn, readiness, bootstrap/login, supervision, and diagnostics.
The workload stays with the caller (`utils/bench/harness-a.sh` today,
a workload wrapper later). The module is importable and stdlib-only; the
CLI in `utils/bench/launch-backend.py` is a thin wrapper.

Environment contract:

  MODE          bench (default) | debug
  OUT           state/output directory; outside the repository. FRESH=1
                additionally requires a path under a temp root
  PORT          server port (default 5713)
  FRESH         1 (default) wipe OUT and bootstrap a new user;
                0 reuse an existing OUT/data and skip bootstrap
  ADMIN_ID      benchmark administrator id (default bench-admin)
  ADMIN_PW      administrator password; generated for FRESH=1, required
                for FRESH=0
  PICASU_BIN    server binary (default target/<profile>/picasu)
  BUILD_PROFILE release (required in bench) | debug
  N             taskset core budget 0-(N-1); 0 disables pinning (default 4)
  READY_TIMEOUT_S  readiness budget in seconds (default 120)
  LAUNCH_TIMEOUT_S outer supervision budget; 0 disables (default 0)
  GDB           1 (default) capture thread stacks into the diag bundle

bench refuses RUST_LOG (log volume perturbs measurements); debug passes env
through unstripped.

Artifacts in OUT: server.log, launcher.log, run-manifest.json,
bootstrap.json, login.json, cookies.txt, launcher-info.json, diag/.
`launcher-info.json` is the seam for a future workload wrapper:
base_url, cookie_jar, pid, out, mode.

Exit codes: 0 completed; 1 config/preflight; 2 startup; 3 reserved;
4 outer timeout.
"""

from __future__ import annotations

import http.cookiejar
import json
import os
import random
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent

MODES = ("bench", "debug")
DIAGNOSTIC_ENV = ("RUST_LOG",)
DEFAULT_PORT = 5713
DEFAULT_ADMIN_ID = "bench-admin"
READY_TIMEOUT_S = 120.0
POLL_INTERVAL_S = 0.2
SERVER_INFO_PATH = "/get/server-info"
BOOTSTRAP_PATH = "/post/users/bootstrap"
AUTH_PATH = "/post/users/authenticate"


class ConfigError(Exception):
    """Invalid or unsafe launcher configuration."""


def log_line(logf, msg: str) -> None:
    line = f"{time.strftime('%H:%M:%S')} {msg}"
    print(line, flush=True)
    logf.write(line + "\n")
    logf.flush()


def port_in_use(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.settimeout(1.0)
        return s.connect_ex(("127.0.0.1", port)) == 0


def port_holder(port: int) -> str:
    ss = shutil.which("ss")
    if ss is None:
        return "(ss unavailable)"
    try:
        out = subprocess.run([ss, "-ltnp"], capture_output=True, text=True, timeout=10).stdout
    except subprocess.SubprocessError, OSError:
        return "(ss failed)"
    return (
        "\n".join(line for line in out.splitlines() if f":{port} " in line) or "(no owner detail)"
    )


def git_info() -> dict:
    def run(*args: str) -> str:
        try:
            out = subprocess.run(
                ["git", "-C", str(REPO_ROOT), *args],
                capture_output=True,
                text=True,
                timeout=15,
            )
        except subprocess.SubprocessError, OSError:
            return "unknown"
        return out.stdout.strip() if out.returncode == 0 else "unknown"

    return {"sha": run("rev-parse", "HEAD"), "status": run("status", "--short")}


# --- config ------------------------------------------------------------------
def allowed_out_roots() -> list:
    """Directories a FRESH wipe is allowed to delete beneath."""
    return [
        Path(tempfile.gettempdir()).resolve(),
        Path("/tmp"),
        Path("/var/tmp"),
        Path("/tmp/opencode"),
    ]


def _resolved_out(raw: str) -> Path:
    if raw == "":
        raise ConfigError("OUT is required")
    # realpath: a pre-existing symlink must not smuggle the guard targets.
    return Path(os.path.realpath(raw))


def _under(path: Path, root: Path) -> bool:
    return path != root and root in path.parents


def _validate_out(raw: str, fresh: bool) -> Path:
    out = _resolved_out(raw)
    if str(out) == "/":
        raise ConfigError("refusing to use OUT='/'")
    if out == REPO_ROOT or REPO_ROOT in out.parents:
        raise ConfigError(f"OUT must not be inside the repo: {out}")
    if fresh:
        roots = allowed_out_roots()
        if not any(_under(out, root) for root in roots):
            shown = ", ".join(str(r) for r in roots)
            raise ConfigError(f"refusing to wipe outside a temp root ({shown}): {out}")
    return out


def _positive_float(environ: dict, name: str, default: float) -> float:
    raw = environ.get(name, str(default))
    try:
        val = float(raw)
    except ValueError:
        raise ConfigError(f"{name} must be a number, got '{raw}'")
    if val <= 0:
        raise ConfigError(f"{name} must be > 0, got {val}")
    return val


def config_from_env(environ: dict) -> dict:
    """Build and validate launcher config. Reads env; only stats paths."""
    mode = environ.get("MODE", "bench")
    if mode not in MODES:
        raise ConfigError(f"MODE must be one of {MODES}, got '{mode}'")

    try:
        port = int(environ.get("PORT", str(DEFAULT_PORT)))
    except ValueError:
        raise ConfigError(f"PORT must be an integer, got '{environ.get('PORT')}'")
    if not 1 <= port <= 65535:
        raise ConfigError(f"PORT must be in 1..65535, got {port}")

    fresh_raw = environ.get("FRESH", "1")
    if fresh_raw not in ("0", "1"):
        raise ConfigError(f"FRESH must be 0 or 1, got '{fresh_raw}'")
    fresh = fresh_raw == "1"
    out = _validate_out(environ.get("OUT", ""), fresh=fresh)

    build_profile = environ.get("BUILD_PROFILE", "release" if mode == "bench" else "debug")
    if build_profile not in ("release", "debug"):
        raise ConfigError(f"BUILD_PROFILE must be release or debug, got '{build_profile}'")
    if mode == "bench" and build_profile != "release":
        raise ConfigError("bench mode requires BUILD_PROFILE=release")
    if mode == "bench" and "RUST_LOG" in environ:
        raise ConfigError("bench mode refuses RUST_LOG (log-volume perturbation)")

    default_bin = REPO_ROOT / "target" / build_profile / "picasu"
    binary = Path(environ.get("PICASU_BIN", str(default_bin)))
    if not (binary.exists() and os.access(binary, os.X_OK)):
        raise ConfigError(f"missing executable server binary {binary}")

    try:
        n = int(environ.get("N", "4"))
    except ValueError:
        raise ConfigError(f"N must be an integer, got '{environ.get('N')}'")
    if n < 0:
        raise ConfigError(f"N must be >= 0, got {n}")

    ready_timeout = _positive_float(environ, "READY_TIMEOUT_S", READY_TIMEOUT_S)
    try:
        launch_timeout = float(environ.get("LAUNCH_TIMEOUT_S", "0"))
    except ValueError:
        raise ConfigError(
            f"LAUNCH_TIMEOUT_S must be a number, got '{environ.get('LAUNCH_TIMEOUT_S')}'"
        )
    if launch_timeout < 0:
        raise ConfigError(f"LAUNCH_TIMEOUT_S must be >= 0, got {launch_timeout}")

    admin_id = environ.get("ADMIN_ID", DEFAULT_ADMIN_ID)
    if fresh:
        generated = f"bench-{os.getpid():04x}-{random.randrange(0x10000):04x}"
        admin_pw = environ.get("ADMIN_PW") or generated
    else:
        admin_pw = environ.get("ADMIN_PW", "")
        if not admin_pw:
            raise ConfigError("FRESH=0 requires ADMIN_PW for the existing data home")
        if not (out / "data").is_dir():
            raise ConfigError(f"FRESH=0 requires an existing data dir: {out / 'data'}")

    return {
        "mode": mode,
        "out": out,
        "port": port,
        "fresh": fresh,
        "binary": binary,
        "build_profile": build_profile,
        "n": n,
        "gdb": environ.get("GDB", "1") != "0",
        "ready_timeout": ready_timeout,
        "launch_timeout": launch_timeout,
        "admin_id": admin_id,
        "admin_pw": admin_pw,
        "base_url": f"http://127.0.0.1:{port}",
        "poll_interval": POLL_INTERVAL_S,
    }


# --- artifacts ---------------------------------------------------------------
def write_manifest(cfg: dict, environ: dict) -> None:
    gi = git_info()
    (cfg["out"] / "run-manifest.json").write_text(
        json.dumps(
            {
                "mode": cfg["mode"],
                "out": str(cfg["out"]),
                "port": cfg["port"],
                "base_url": cfg["base_url"],
                "fresh": cfg["fresh"],
                "build_profile": cfg["build_profile"],
                "N": environ.get("N", ""),
                "ADMIN_ID": cfg["admin_id"],
                "RUST_LOG": environ.get("RUST_LOG", ""),
                "git": gi["sha"],
                "started": datetime.now(timezone.utc).isoformat(),
            },
            indent=2,
        )
        + "\n"
    )


def write_launcher_info(cfg: dict, pid: int) -> None:
    (cfg["out"] / "launcher-info.json").write_text(
        json.dumps(
            {
                "base_url": cfg["base_url"],
                "cookie_jar": str(cfg["out"] / "cookies.txt"),
                "pid": pid,
                "out": str(cfg["out"]),
                "mode": cfg["mode"],
            },
            indent=2,
        )
        + "\n"
    )


# --- HTTP --------------------------------------------------------------------
def probe_server_info(base_url: str, timeout: float = 2.0) -> bool:
    try:
        with urllib.request.urlopen(base_url + SERVER_INFO_PATH, timeout=timeout) as resp:
            return resp.status == 200
    except urllib.error.HTTPError as err:
        err.close()
        return False
    except urllib.error.URLError, OSError:
        return False


def _opener(jar=None):
    handlers = [urllib.request.ProxyHandler({})]
    if jar is not None:
        handlers.append(urllib.request.HTTPCookieProcessor(jar))
    return urllib.request.build_opener(*handlers)


def _post_json(opener, url: str, payload: dict, timeout: float = 10.0):
    body = json.dumps(payload).encode()
    req = urllib.request.Request(
        url, data=body, headers={"Content-Type": "application/json"}, method="POST"
    )
    try:
        with opener.open(req, timeout=timeout) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as err:
        body = err.read()
        err.close()
        return err.code, body
    except (urllib.error.URLError, OSError) as err:
        raise OSError(f"request to {url} failed: {err}")


def _body_text(raw: bytes) -> str:
    return raw.decode(errors="replace").strip()


# --- process control ---------------------------------------------------------
def server_command(cfg: dict) -> list:
    cmd = [str(cfg["binary"])]
    if cfg["n"] > 0 and shutil.which("taskset") is not None:
        cmd = ["taskset", "-c", f"0-{cfg['n'] - 1}", *cmd]
    return cmd


def server_env(cfg: dict, environ: dict) -> dict:
    out = cfg["out"]
    data = out / "data"
    env = dict(environ)
    env.update(
        {
            "PICASU_ADDRESS": "127.0.0.1",
            "PICASU_PORT": str(cfg["port"]),
            "PICASU_DATA_HOME": str(data),
            "PICASU_CONFIG_HOME": str(out / "config"),
            "PICASU_IMAGE_HOME": str(out / "images"),
            "PICASU_USERS_HOME": str(data / "users"),
            "PICASU_BOOTSTRAP_ADMIN": "true" if cfg["fresh"] else "false",
            "PICASU_TRIVIAL_PASSWORDS": "true",
            "PICASU_WEAK_PWHASH": "1",
        }
    )
    if cfg["mode"] == "bench":
        for name in DIAGNOSTIC_ENV:
            env.pop(name, None)
    return env


def spawn_server(cfg: dict, env: dict):
    """Start the server in its own process group; stdout/stderr -> server.log."""
    out = cfg["out"]
    mode = "w" if cfg["fresh"] else "a"
    with open(out / "server.log", mode) as logf:  # noqa: PTH123
        return subprocess.Popen(
            server_command(cfg),
            stdout=logf,
            stderr=subprocess.STDOUT,
            start_new_session=True,
            env=env,
        )


def kill_group(proc, grace: float = 5.0) -> None:
    def signal_group(sig) -> None:
        try:
            os.killpg(os.getpgid(proc.pid), sig)
        except ProcessLookupError, PermissionError, OSError:
            try:
                proc.send_signal(sig)
            except ProcessLookupError, OSError:
                pass

    if proc.poll() is not None:
        return
    signal_group(signal.SIGTERM)
    deadline = time.monotonic() + grace
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            return
        time.sleep(0.1)
    signal_group(signal.SIGKILL)


# --- diagnostics -------------------------------------------------------------
def _tail(path: Path, lines: int) -> str:
    try:
        return "\n".join(path.read_text(errors="replace").splitlines()[-lines:])
    except OSError:
        return ""


SYSTEM_GDB = "/usr/bin/gdb"
GDB_TIMEOUT_S = 120.0


def _rust_pretty_printers() -> tuple:
    """Return `(gdb_args, env)` that enable the toolchain's Rust pretty-printers.

    Reproduces the setup `rust-gdb` performs for batch use: source directory,
    auto-load safe path, and `PYTHONPATH` for `gdb_load_rust_pretty_printers.py`.
    Empty when no toolchain `etc` directory is available, so a plain gdb still
    produces backtraces.
    """
    rustc = shutil.which("rustc")
    if rustc is None:
        return [], {}
    try:
        probe = subprocess.run(
            [rustc, "--print=sysroot"], capture_output=True, text=True, timeout=15
        )
    except subprocess.SubprocessError, OSError:
        return [], {}
    if probe.returncode != 0:
        return [], {}
    etc = Path(probe.stdout.strip()) / "lib" / "rustlib" / "etc"
    if not (etc / "gdb_load_rust_pretty_printers.py").is_file():
        return [], {}
    args = [f"--directory={etc}", "-iex", f"add-auto-load-safe-path {etc}"]
    env = {"PYTHONPATH": f"{etc}:{os.environ.get('PYTHONPATH', '')}".rstrip(":")}
    return args, env


def gdb_candidates() -> list:
    """gdb binaries to try, best first.

    The system gdb comes first because it can carry a file capability such as
    CAP_SYS_PTRACE, which the rustup `rust-gdb` indirection cannot add. Both
    remain candidates so a capability-less system gdb degrades to the
    toolchain wrapper or a PATH gdb.
    """
    candidates = []
    if os.path.isfile(SYSTEM_GDB) and os.access(SYSTEM_GDB, os.X_OK):
        candidates.append(SYSTEM_GDB)
    for name in ("rust-gdb", "gdb"):
        path = shutil.which(name)
        if path and path not in candidates:
            candidates.append(path)
    return candidates


def capture_stacks(pid: int, diag: Path, logf, use_gdb: bool) -> dict:
    """Write `threads.txt` and, when gdb can attach, `stacks.txt`.

    Tries `gdb_candidates()` in order and keeps the last failure output in
    `stacks.txt`. Returns a summary for the orchestrator's `captures` dict,
    including the reason stack capture degraded.
    """
    result = {"ok": False, "tool": None, "reason": None, "attempts": []}
    try:
        nthreads = len(list((Path(f"/proc/{pid}/task")).iterdir()))
    except OSError:
        log_line(logf, f"stack capture: pid {pid} gone")
        result["reason"] = "process gone"
        return result
    (diag / "threads.txt").write_text(f"{nthreads}\n")
    result["threads"] = nthreads

    if not use_gdb:
        log_line(logf, "stack capture skipped (GDB off)")
        result["reason"] = "GDB disabled (GDB=0)"
        return result
    candidates = gdb_candidates()
    if not candidates:
        log_line(logf, "stack capture skipped (no gdb binary found)")
        result["reason"] = "no gdb binary found (tried /usr/bin/gdb, rust-gdb, gdb)"
        return result

    pretty_args, pretty_env = _rust_pretty_printers()
    env = dict(os.environ)
    env.update(pretty_env)
    failure = ""
    for tool in candidates:
        try:
            out = subprocess.run(
                [
                    tool,
                    *pretty_args,
                    "-batch",
                    "-ex",
                    "thread apply all bt",
                    "-p",
                    str(pid),
                ],
                capture_output=True,
                text=True,
                timeout=GDB_TIMEOUT_S,
                env=env,
            )
        except (subprocess.SubprocessError, OSError) as err:
            log_line(logf, f"gdb stack capture via {tool} failed: {err}")
            result["attempts"].append({"tool": tool, "error": str(err)})
            failure = f"{tool}: {err}"
            continue

        text = out.stdout + out.stderr
        frames = sum(1 for line in text.splitlines() if line.startswith("Thread "))
        result["attempts"].append({"tool": tool, "rc": out.returncode, "threads": frames})
        if out.returncode == 0 and frames > 0:
            (diag / "stacks.txt").write_text(text)
            log_line(logf, f"stacks: {frames} threads via {tool} -> {diag / 'stacks.txt'}")
            result.update(ok=True, tool=tool, frames=frames)
            return result

        # Preserve the failure output for the bundle, then try the next tool.
        (diag / "stacks.txt").write_text(text)
        failure = f"{tool}: rc={out.returncode}, {frames} thread frames"
        log_line(logf, f"gdb stack capture via {tool} produced {frames} thread frames; trying next")

    result["reason"] = f"all gdb candidates failed ({failure})"
    log_line(logf, f"stack capture degraded: {result['reason']}")
    return result


def collect_bundle(cfg: dict, proc, reason: str, logf) -> None:
    diag = cfg["out"] / "diag"
    diag.mkdir(parents=True, exist_ok=True)
    log_line(logf, f"collecting diagnostics ({reason}) into {diag}")
    for name in ("status-last.json", "run-manifest.json", "launcher-info.json"):
        src = cfg["out"] / name
        if src.exists():
            shutil.copy(src, diag / name.replace(".json", f"-{reason}.json"))
    tail = _tail(cfg["out"] / "server.log", 200)
    if tail:
        (diag / "server-tail.log").write_text(tail + "\n")
    capture_stacks(proc.pid, diag, logf, cfg["gdb"])


# --- startup steps -----------------------------------------------------------
def prepare_out(cfg: dict) -> None:
    out = cfg["out"]
    if cfg["fresh"]:
        if out.exists():
            shutil.rmtree(out)
        out.mkdir(parents=True)
    else:
        out.mkdir(parents=True, exist_ok=True)
    for sub in ("data", "data/users", "config", "images"):
        (out / sub).mkdir(parents=True, exist_ok=True)


def wait_ready(cfg: dict, proc):
    """Poll /get/server-info; distinguish ready, server death, and timeout."""
    deadline = time.monotonic() + cfg["ready_timeout"]
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            return "died"
        if probe_server_info(cfg["base_url"]):
            return "ready"
        time.sleep(cfg["poll_interval"])
    return "timeout"


def bootstrap(cfg: dict, logf) -> None:
    opener = _opener()
    code, raw = _post_json(
        opener,
        cfg["base_url"] + BOOTSTRAP_PATH,
        {"userId": cfg["admin_id"], "password": cfg["admin_pw"]},
    )
    (cfg["out"] / "bootstrap.json").write_text(_body_text(raw) + "\n")
    if code != 200:
        body = _body_text(raw)
        log_line(logf, f"bootstrap returned {code}: {body}")
        raise RuntimeError(
            f"bootstrap returned {code}. The bootstrap window was closed; "
            f"if the data dir is not fresh, set FRESH=0 and supply ADMIN_PW. {body}"
        )
    log_line(logf, f"bootstrap=200 {_body_text(raw)}")


def authenticate(cfg: dict, logf) -> None:
    jar_path = cfg["out"] / "cookies.txt"
    jar_path.unlink(missing_ok=True)
    jar = http.cookiejar.MozillaCookieJar(str(jar_path))
    opener = _opener(jar)
    code, raw = _post_json(
        opener,
        cfg["base_url"] + AUTH_PATH,
        {"userId": cfg["admin_id"], "password": cfg["admin_pw"]},
    )
    (cfg["out"] / "login.json").write_text(_body_text(raw) + "\n")
    if code != 200:
        log_line(logf, f"login returned {code}: {_body_text(raw)}")
        raise RuntimeError(f"login returned {code}")
    jar.save(ignore_discard=True, ignore_expires=True)
    try:
        cookies = jar_path.read_text()
    except OSError:
        cookies = ""
    if "jwt" not in cookies:
        raise RuntimeError("login set no jwt cookie")
    log_line(logf, f"login=200 cookie_jar={jar_path}")


def supervise(cfg: dict, proc) -> str:
    """Wait for server exit or the optional outer timeout."""
    start = time.monotonic()
    while True:
        if proc.poll() is not None:
            return "exited"
        if cfg["launch_timeout"] and time.monotonic() - start >= cfg["launch_timeout"]:
            return "timeout"
        time.sleep(cfg["poll_interval"])


# --- main flow ---------------------------------------------------------------
def run_backend(cfg: dict, environ: dict, spawn=None) -> int:
    """Prepare, spawn, authenticate, supervise. Returns a launcher exit code."""
    spawn = spawn or spawn_server
    out = cfg["out"]

    # Preflight before touching OUT: a busy port must fail fast and leave
    # whatever state is already there alone.
    if port_in_use(cfg["port"]):
        print(
            f"ERROR: port {cfg['port']} already in use:\n{port_holder(cfg['port'])}\n"
            "Pick a free PORT or stop the holder above. Not killing anything.",
            file=sys.stderr,
        )
        return 1

    try:
        prepare_out(cfg)
    except OSError as err:
        print(f"ERROR: preparing OUT failed: {err}", file=sys.stderr)
        return 2

    logf = open(out / "launcher.log", "w" if cfg["fresh"] else "a")  # noqa: PTH123
    proc = None
    try:
        log_line(logf, "=== backend launcher preflight ===")
        log_line(
            logf,
            f"mode={cfg['mode']} fresh={cfg['fresh']} out={out} port={cfg['port']} "
            f"build_profile={cfg['build_profile']} n={cfg['n']} gdb={cfg['gdb']}",
        )
        gi = git_info()
        log_line(logf, f"git={gi['sha'][:12]} {gi['status']}")

        write_manifest(cfg, environ)

        try:
            proc = spawn(cfg, server_env(cfg, environ))
        except OSError as err:
            log_line(logf, f"server spawn failed: {err}")
            return 2
        log_line(logf, f"server pid={proc.pid} log={out / 'server.log'}")

        state = wait_ready(cfg, proc)
        if state == "died":
            tail = _tail(out / "server.log", 30)
            log_line(logf, f"server exited during startup (rc={proc.poll()})")
            if tail:
                log_line(logf, tail)
            collect_bundle(cfg, proc, "startup-died", logf)
            return 2
        if state == "timeout":
            log_line(logf, f"server not ready within {cfg['ready_timeout']}s")
            collect_bundle(cfg, proc, "ready-timeout", logf)
            kill_group(proc)
            return 2
        log_line(logf, f"server ready pid={proc.pid} base_url={cfg['base_url']}")

        try:
            if cfg["fresh"]:
                bootstrap(cfg, logf)
            else:
                log_line(logf, "FRESH=0: skipping bootstrap, reusing existing data home")
            authenticate(cfg, logf)
        except OSError as err:
            log_line(logf, f"auth request failed: {err}")
            collect_bundle(cfg, proc, "auth-failed", logf)
            return 2
        except RuntimeError as err:
            log_line(logf, f"startup auth failed: {err}")
            collect_bundle(cfg, proc, "auth-failed", logf)
            return 2

        write_launcher_info(cfg, proc.pid)

        state = supervise(cfg, proc)
        if state == "timeout":
            log_line(logf, f"outer timeout {cfg['launch_timeout']}s reached")
            collect_bundle(cfg, proc, "launch-timeout", logf)
            kill_group(proc)
            return 4

        rc = proc.poll()
        if rc == 0:
            log_line(logf, f"server exited cleanly: {out}")
            return 0
        log_line(logf, f"server exited rc={rc}")
        collect_bundle(cfg, proc, f"server-exited-{rc}", logf)
        return 2
    finally:
        if proc is not None and proc.poll() is None:
            kill_group(proc)
        logf.close()
