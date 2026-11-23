#!/usr/bin/env python3
"""Typed, validated configuration for the benchmark harness.

Every harness layer builds exactly one dataclass from its inputs and
consumes attributes from then on:

    LauncherConfig   backend_launcher.py  server process half of a cell
    WorkloadConfig   workload.py          measurement half of a cell
    CellConfig       run_cell.py          one cell: launcher + workload
    BatteryConfig    battery.py           one battery run (knobs + budgets)
    PrepareConfig    prepare.py           `bench prepare` inputs
    PreflightConfig  preflight.py         `bench preflight` inputs

`from_env` (and `from_args` for the flag-only prepare step) is the single
constructor: it applies every default and validation rule. Failures raise
`ConfigError`, so one bad variable is rejected with one message wherever
it is read. Layer-specific seams stay with the layer: the launcher and
orchestrator bind their own repository root and scripts.

Defaults live here because they are part of the validated contract; the
layer modules import the ones they also use at runtime.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import shutil
import tempfile
import urllib.parse
from dataclasses import dataclass, field
from pathlib import Path

import scenarios

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent

DEFAULT_PORT = 5713
DEFAULT_ADMIN_ID = "bench-admin"
READY_TIMEOUT_S = 120.0
CELL_READY_TIMEOUT_S = 150.0
CELL_STOP_TIMEOUT_S = 30.0
POLL_INTERVAL_S = 0.2

DEFAULT_SETS_ROOT = Path.home() / "picasu-bench"
BATTERY_BASE_PORT = 5795
BATTERY_REPEATS_DEFAULT = 3
BATTERY_REPEATS_MIN = 1
DEFAULT_MIN_PROBE_ROUNDS = 20
# The 2 h budget covers the measured ~1335 s single probe cell at 5650 rows.

DEFAULT_IMAGES = 100
DEFAULT_REPEATS = 3
DEFAULT_WARMUP = 1
DEFAULT_RUNS_DURING = 3
DEFAULT_SCAN_REPEATS = 3
DEFAULT_FETCHES = 10
DEFAULT_PROBE_PAGE = 1000
DEFAULT_INDEX_SETTLE_TIMEOUT_S = 600.0
DEFAULT_UPLOAD_TIMEOUT_S = 120.0
DEFAULT_UPLOAD_POST_TIMEOUT_S = 60.0
DEFAULT_STORM_CLIENTS = 8
DEFAULT_STORM_OPS = 10
DEFAULT_UPLOAD_FILES = 20
DEFAULT_STORM_MIN_COVERAGE_PCT = 50.0
DEFAULT_POLL_INTERVAL_S = 0.2
DEFAULT_PROBE_INTERVAL_S = 0.5
DEFAULT_MIN_VALID_ROUNDS = 3
DEFAULT_SCAN_WARMUP = 1
DEFAULT_PROBE_REFRESH_S = 0.0
DEFAULT_BUILD_PROFILE = "debug"

# The scenario table is the single source of truth for the cell matrix.
WORKLOADS = scenarios.profiles()


class ConfigError(Exception):
    """Invalid or unsafe harness configuration."""


# --- shared validators -------------------------------------------------------
def integer(environ: dict, name: str, default: int, minimum: int | None = None) -> int:
    """Read an integer variable, optionally refusing values below `minimum`."""
    raw = environ.get(name, str(default))
    try:
        value = int(raw)
    except TypeError, ValueError:
        raise ConfigError(f"{name} must be an integer, got '{raw}'")
    if minimum is not None and value < minimum:
        raise ConfigError(f"{name} must be >= {minimum}, got {value}")
    return value


def number(
    environ: dict,
    name: str,
    default: float,
    minimum: float = 0.0,
    *,
    exclusive: bool = False,
) -> float:
    """Read a float variable, refusing values outside the bound.

    `exclusive` turns the bound into `value > minimum` and reports it as
    `must be > N`; otherwise `value >= minimum`.
    """
    raw = environ.get(name, str(default))
    try:
        value = float(raw)
    except TypeError, ValueError:
        raise ConfigError(f"{name} must be a number, got '{raw}'")
    if exclusive:
        if value <= minimum:
            raise ConfigError(f"{name} must be > {minimum:g}, got {value}")
    elif value < minimum:
        raise ConfigError(f"{name} must be >= {minimum}, got {value}")
    return value


def validate_port(raw) -> int:
    """Parse and range-check PORT; shared with the preflight checklist."""
    try:
        port = int(raw)
    except TypeError, ValueError:
        raise ConfigError(f"PORT must be an integer, got '{raw}'")
    if not 1 <= port <= 65535:
        raise ConfigError(f"PORT must be in 1..65535, got {port}")
    return port


def validate_build_profile(environ: dict, default: str) -> str:
    profile = environ.get("BUILD_PROFILE", default)
    if profile not in ("release", "debug"):
        raise ConfigError(f"BUILD_PROFILE must be release or debug, got '{profile}'")
    return profile


def allowed_out_roots() -> list:
    """Directories a FRESH wipe is allowed to delete beneath."""
    return [
        Path(tempfile.gettempdir()).resolve(),
        Path("/tmp"),
        Path("/var/tmp"),
        Path("/tmp/opencode"),
        Path("/dev/shm"),
    ]


def _under(path: Path, root: Path) -> bool:
    return path != root and root in path.parents


def validate_out(
    raw: str,
    fresh: bool,
    *,
    repo_root: Path = REPO_ROOT,
    required: str = "OUT is required",
) -> Path:
    """Resolve and safety-check OUT; `fresh` additionally requires a temp root."""
    if raw == "":
        raise ConfigError(required)
    # realpath: a pre-existing symlink must not smuggle the guard targets.
    out = Path(os.path.realpath(raw))
    if str(out) == "/":
        raise ConfigError("refusing to use OUT='/'")
    if out == repo_root or repo_root in out.parents:
        raise ConfigError(f"OUT must not be inside the repo: {out}")
    if fresh:
        roots = allowed_out_roots()
        if not any(_under(out, root) for root in roots):
            shown = ", ".join(str(r) for r in roots)
            raise ConfigError(f"refusing to wipe outside a temp root ({shown}): {out}")
    return out


def executable_file(path: Path) -> bool:
    return path.exists() and os.access(path, os.X_OK)


def resolve_binary(environ: dict, repo_root: Path = REPO_ROOT) -> tuple:
    """Return `(binary, explicit_bin, build_profile)` for this cell.

    An explicit PICASU_BIN is used as-is; otherwise the launcher uses
    target/<profile>/picasu. Existence is not checked here.
    """
    build_profile = validate_build_profile(environ, "release")
    explicit_bin = "PICASU_BIN" in environ
    if explicit_bin:
        binary = Path(environ["PICASU_BIN"])
    else:
        binary = repo_root / "target" / build_profile / "picasu"
    return binary, explicit_bin, build_profile


def validate_image_home_src(raw: str) -> Path:
    """Return the prepared sandbox a cell will read images from.

    A sandbox is only a sandbox when `bench prepare` wrote its manifest.
    """
    image_home_path = Path(raw)
    if not image_home_path.is_dir():
        raise ConfigError(f"IMAGE_HOME_SRC must be a directory, got '{raw}'")
    if not (image_home_path / "sandbox-manifest.json").is_file():
        raise ConfigError(
            f"IMAGE_HOME_SRC has no sandbox-manifest.json: {raw} (prepare it with `bench prepare`)"
        )
    return image_home_path


def load_launcher_info(path: Path) -> dict:
    try:
        info = json.loads(path.read_text())
    except OSError as err:
        raise ConfigError(f"cannot read launcher info {path}: {err}")
    except json.JSONDecodeError as err:
        raise ConfigError(f"invalid launcher info {path}: {err}")
    for key in ("base_url", "cookie_jar", "out"):
        if not info.get(key):
            raise ConfigError(f"launcher info missing '{key}': {path}")
    return info


# --- launcher layer ----------------------------------------------------------
@dataclass(frozen=True)
class LauncherConfig:
    """The server half of one cell."""

    mode: str
    out: Path
    port: int
    fresh: bool
    binary: Path
    explicit_bin: bool
    build_profile: str
    ncpu: int
    gdb: bool
    ready_timeout: float
    launch_timeout: float
    admin_id: str
    admin_pw: str
    base_url: str
    poll_interval: float
    image_home: Path | None
    image_home_src: str | None

    @classmethod
    def from_env(cls, environ: dict, *, repo_root: Path = REPO_ROOT) -> LauncherConfig:
        port = validate_port(environ.get("PORT", str(DEFAULT_PORT)))

        fresh_raw = environ.get("FRESH", "1")
        if fresh_raw not in ("0", "1"):
            raise ConfigError(f"FRESH must be 0 or 1, got '{fresh_raw}'")
        fresh = fresh_raw == "1"
        out = validate_out(environ.get("OUT", ""), fresh, repo_root=repo_root)

        if "IMAGE_HOME_SRC" in environ:
            image_home_src = environ["IMAGE_HOME_SRC"]
            image_home = validate_image_home_src(image_home_src)
        else:
            image_home = None
            image_home_src = None

        binary, explicit_bin, build_profile = resolve_binary(environ, repo_root=repo_root)
        # No MODE knob: a cell is canonical ("bench") iff it runs the release
        # binary with default logging; anything else is "debug".
        mode = "bench" if (build_profile == "release" and "RUST_LOG" not in environ) else "debug"

        if not executable_file(binary):
            raise ConfigError(
                f"missing executable server binary {binary} "
                f"(build it: cargo build --profile {build_profile} --bin picasu)"
            )

        ncpu = integer(environ, "NCPU", 4, minimum=0)
        ready_timeout = number(environ, "READY_TIMEOUT_S", READY_TIMEOUT_S, exclusive=True)
        launch_timeout = number(environ, "LAUNCH_TIMEOUT_S", 0.0)

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

        return cls(
            mode=mode,
            out=out,
            port=port,
            fresh=fresh,
            binary=binary,
            explicit_bin=explicit_bin,
            build_profile=build_profile,
            ncpu=ncpu,
            gdb=environ.get("GDB", "1") != "0",
            ready_timeout=ready_timeout,
            launch_timeout=launch_timeout,
            admin_id=admin_id,
            admin_pw=admin_pw,
            base_url=f"http://127.0.0.1:{port}",
            poll_interval=POLL_INTERVAL_S,
            image_home=image_home,
            image_home_src=image_home_src,
        )


# --- workload layer ----------------------------------------------------------
@dataclass
class WorkloadConfig:
    """The measurement half of one cell.

    Fields default to the documented environment defaults so a partial
    object can drive one helper in tests; `from_env` sets every field.
    `image_count` and the scan totals are results recorded by
    `run_workload`, not inputs.
    """

    out: Path
    launcher_info: dict = field(default_factory=dict)
    base_url: str = ""
    cookie_jar: Path | None = None
    ncpu: int = 4
    port: int = DEFAULT_PORT
    images: int = DEFAULT_IMAGES
    workload: str = scenarios.PROBE
    storm_clients: int = DEFAULT_STORM_CLIENTS
    storm_ops: int = DEFAULT_STORM_OPS
    storm_min_coverage_pct: float = DEFAULT_STORM_MIN_COVERAGE_PCT
    scan_repeats: int = DEFAULT_SCAN_REPEATS
    upload_files: int = DEFAULT_UPLOAD_FILES
    upload_repo: Path | None = None
    upload_timeout_s: float = DEFAULT_UPLOAD_TIMEOUT_S
    upload_post_timeout_s: float = DEFAULT_UPLOAD_POST_TIMEOUT_S
    index_settle_timeout_s: float = DEFAULT_INDEX_SETTLE_TIMEOUT_S
    repeats: int = DEFAULT_REPEATS
    min_probe_rounds: int = DEFAULT_MIN_PROBE_ROUNDS
    warmup: int = DEFAULT_WARMUP
    runs_during: int = DEFAULT_RUNS_DURING
    fetches: int = DEFAULT_FETCHES
    probe_limit: int = DEFAULT_IMAGES
    probe_page: int = DEFAULT_PROBE_PAGE
    scan_timeout_s: float = 0.0
    scan_stall_timeout_s: float = 120.0
    poll_interval_s: float = DEFAULT_POLL_INTERVAL_S
    probe_interval_s: float = DEFAULT_PROBE_INTERVAL_S
    min_valid_rounds: int = DEFAULT_MIN_VALID_ROUNDS
    scan_warmup: int = DEFAULT_SCAN_WARMUP
    probe_refresh_s: float = DEFAULT_PROBE_REFRESH_S
    image_repo: Path | None = None
    image_path: Path | None = None
    image_home: str | None = None
    dataset_id: str = ""
    build_profile: str = DEFAULT_BUILD_PROFILE
    # Results recorded while the workload runs.
    image_count: int = 0
    image_bytes: int = 0
    expected_entries: int = 0
    scan_wall_ms: float = 0.0
    scan_runs: int = 0
    scan_walls_ms: list = field(default_factory=list)

    @classmethod
    def from_env(cls, environ: dict) -> WorkloadConfig:
        """Build from the environment."""
        for old, new in (
            ("N", "NCPU"),
            ("COUNT", "IMAGES"),
            ("REUSE_IMAGES", "IMAGE_REPO"),
            ("SKIP_GENERATE", None),
            ("GEN_JOBS", None),
            ("SNAPFAB_BIN", None),
            ("MANIFEST", None),
        ):
            if old in environ:
                hint = f"use {new} instead" if new else "seeding moved to `just bench seed-images`"
                raise ConfigError(f"{old} was removed ({hint})")

        if not environ.get("IMAGE_REPO", ""):
            raise ConfigError("IMAGE_REPO is required (seed a repo with `just bench seed-images`)")

        info_path = environ.get("LAUNCHER_INFO", "")
        if not info_path:
            raise ConfigError("LAUNCHER_INFO is required")
        info = load_launcher_info(Path(info_path))

        base_url = str(info["base_url"]).rstrip("/")
        cookie_jar = Path(info["cookie_jar"])
        if not cookie_jar.is_file():
            raise ConfigError(f"cookie jar not found: {cookie_jar}")

        out = validate_out(environ.get("OUT") or str(info["out"]), False)

        try:
            port = urllib.parse.urlsplit(base_url).port or DEFAULT_PORT
        except ValueError:
            raise ConfigError(f"invalid base_url: {base_url}")
        # The orchestrator passes the cell's PORT; when present it is the
        # same value the launcher used, and it is range-checked here too so
        # all three layers reject a bad PORT identically. The server port
        # itself stays launcher-info's base_url.
        if "PORT" in environ:
            validate_port(environ["PORT"])

        ncpu = integer(environ, "NCPU", 4, minimum=0)
        build_profile = validate_build_profile(environ, DEFAULT_BUILD_PROFILE)

        images = integer(environ, "IMAGES", DEFAULT_IMAGES, minimum=1)
        workload = environ.get("WORKLOAD", scenarios.PROBE)
        if workload not in WORKLOADS:
            raise ConfigError(f"WORKLOAD must be one of {WORKLOADS}, got '{workload}'")
        if "STORM_CLIENTS" in environ:
            storm_clients = integer(environ, "STORM_CLIENTS", DEFAULT_STORM_CLIENTS, minimum=1)
        elif scenarios.is_storm(workload):
            _background, _kind, level = scenarios.storm_parts(workload)
            storm_clients = scenarios.level_clients(level, ncpu)
        else:
            storm_clients = DEFAULT_STORM_CLIENTS
        storm_ops = integer(environ, "STORM_OPS", DEFAULT_STORM_OPS, minimum=1)
        storm_min_coverage = number(
            environ, "STORM_MIN_COVERAGE_PCT", DEFAULT_STORM_MIN_COVERAGE_PCT, minimum=1.0
        )
        if storm_min_coverage > 100.0:
            raise ConfigError(f"STORM_MIN_COVERAGE_PCT must be <= 100, got {storm_min_coverage}")
        scan_repeats = integer(environ, "SCAN_REPEATS", DEFAULT_SCAN_REPEATS, minimum=1)
        upload_files = integer(environ, "UPLOAD_FILES", DEFAULT_UPLOAD_FILES, minimum=1)
        upload_repo = None
        if environ.get("UPLOAD_REPO"):
            upload_repo = Path(environ["UPLOAD_REPO"])
            if not upload_repo.is_dir():
                raise ConfigError(f"UPLOAD_REPO not a directory: {upload_repo}")
        upload_timeout = number(environ, "UPLOAD_TIMEOUT_S", DEFAULT_UPLOAD_TIMEOUT_S, minimum=0.1)
        upload_post_timeout = number(
            environ, "UPLOAD_POST_TIMEOUT_S", DEFAULT_UPLOAD_POST_TIMEOUT_S, minimum=0.1
        )
        index_settle_timeout = number(
            environ, "INDEX_SETTLE_TIMEOUT_S", DEFAULT_INDEX_SETTLE_TIMEOUT_S, minimum=0.1
        )
        repeats = integer(environ, "REPEATS", DEFAULT_REPEATS, minimum=1)
        min_probe_rounds = integer(environ, "MIN_PROBE_ROUNDS", DEFAULT_MIN_PROBE_ROUNDS, minimum=1)
        warmup = integer(environ, "WARMUP", DEFAULT_WARMUP, minimum=0)
        runs_during = integer(environ, "RUNS_DURING", DEFAULT_RUNS_DURING, minimum=1)
        fetches = integer(environ, "FETCHES", DEFAULT_FETCHES, minimum=0)
        probe_limit = integer(environ, "PROBE_LIMIT", images, minimum=1)
        probe_page = integer(environ, "PROBE_PAGE", DEFAULT_PROBE_PAGE, minimum=1)
        scan_timeout = number(environ, "SCAN_TIMEOUT_S", 0.0)
        scan_stall_timeout = number(environ, "SCAN_STALL_TIMEOUT_S", 120.0)
        poll_interval = number(environ, "POLL_INTERVAL_S", DEFAULT_POLL_INTERVAL_S, minimum=0.001)
        probe_interval = number(
            environ, "PROBE_INTERVAL_S", DEFAULT_PROBE_INTERVAL_S, minimum=0.001
        )
        min_valid_rounds = integer(environ, "MIN_VALID_ROUNDS", DEFAULT_MIN_VALID_ROUNDS, minimum=1)
        scan_warmup = integer(environ, "SCAN_WARMUP", DEFAULT_SCAN_WARMUP, minimum=0)
        probe_refresh = number(environ, "PROBE_REFRESH_S", DEFAULT_PROBE_REFRESH_S, minimum=0.0)

        repo_path = Path(environ["IMAGE_REPO"])
        if not repo_path.is_dir():
            raise ConfigError(f"IMAGE_REPO not a directory: {environ['IMAGE_REPO']}")

        image_home_raw = info.get("image_home")
        if environ.get("IMAGE_PATH", ""):
            image_path = Path(environ["IMAGE_PATH"])
        elif image_home_raw:
            image_path = Path(image_home_raw)
        else:
            image_path = out / "images"

        dataset_id = environ.get("DATASET_ID", "")
        if not dataset_id:
            if image_home_raw:
                dataset_id = Path(image_home_raw).name
            else:
                dataset_id = "out-local"

        return cls(
            launcher_info=info,
            base_url=base_url,
            cookie_jar=cookie_jar,
            out=out,
            ncpu=ncpu,
            port=port,
            images=images,
            workload=workload,
            storm_clients=storm_clients,
            storm_ops=storm_ops,
            storm_min_coverage_pct=storm_min_coverage,
            scan_repeats=scan_repeats,
            upload_files=upload_files,
            upload_repo=upload_repo,
            upload_timeout_s=upload_timeout,
            upload_post_timeout_s=upload_post_timeout,
            index_settle_timeout_s=index_settle_timeout,
            repeats=repeats,
            min_probe_rounds=min_probe_rounds,
            warmup=warmup,
            runs_during=runs_during,
            fetches=fetches,
            probe_limit=probe_limit,
            probe_page=probe_page,
            scan_timeout_s=scan_timeout,
            scan_stall_timeout_s=scan_stall_timeout,
            poll_interval_s=poll_interval,
            probe_interval_s=probe_interval,
            min_valid_rounds=min_valid_rounds,
            scan_warmup=scan_warmup,
            probe_refresh_s=probe_refresh,
            image_repo=repo_path,
            image_path=image_path,
            image_home=image_home_raw,
            dataset_id=dataset_id,
            build_profile=build_profile,
        )


# --- cell layer --------------------------------------------------------------
@dataclass(frozen=True)
class CellConfig:
    """One cell: the orchestrator's view of its launcher and workload."""

    out: Path
    port: int
    build_profile: str
    ready_timeout: float
    stop_timeout: float
    workload_timeout: float
    gdb: bool
    poll_interval: float
    launcher_script: Path
    workload_script: Path

    @classmethod
    def from_env(
        cls,
        environ: dict,
        *,
        out_flag=None,
        port_flag=None,
        which=shutil.which,
    ) -> CellConfig:
        """Build from the environment; `out_flag` and `port_flag` win over it."""
        out = validate_out(
            out_flag if out_flag is not None else environ.get("OUT", ""),
            False,
            required="OUT is required (environment OUT or --out)",
        )
        raw_port = port_flag if port_flag is not None else environ.get("PORT", "")
        if raw_port == "":
            raise ConfigError("PORT is required (environment PORT or --port)")
        port = validate_port(str(raw_port))

        build_profile = validate_build_profile(environ, "release")

        return cls(
            out=out,
            port=port,
            build_profile=build_profile,
            ready_timeout=number(
                environ, "LAUNCHER_READY_TIMEOUT_S", CELL_READY_TIMEOUT_S, exclusive=True
            ),
            stop_timeout=number(
                environ, "LAUNCHER_STOP_TIMEOUT_S", CELL_STOP_TIMEOUT_S, exclusive=True
            ),
            workload_timeout=number(environ, "WORKLOAD_TIMEOUT_S", 0.0),
            gdb=environ.get("GDB", "1") != "0",
            poll_interval=POLL_INTERVAL_S,
            launcher_script=SCRIPT_DIR / "backend_launcher.py",
            workload_script=SCRIPT_DIR / "workload.py",
        )


# --- battery layer -----------------------------------------------------------
@dataclass(frozen=True)
class BatteryConfig:
    """The knobs and budgets of one battery run."""

    images: int = DEFAULT_IMAGES
    out: Path = Path(".")
    sets_root: Path = DEFAULT_SETS_ROOT
    base_port: int = BATTERY_BASE_PORT
    build_profile: str = "release"
    fresh: bool = False
    repeats: int = BATTERY_REPEATS_DEFAULT
    min_probe_rounds: int = DEFAULT_MIN_PROBE_ROUNDS
    # Forwarded verbatim to run_cell, which owns its validation.
    workload_timeout_s: str = "1800"

    @classmethod
    def from_env(
        cls,
        environ: dict,
        *,
        images: int | None = None,
        out,
        sets_root=DEFAULT_SETS_ROOT,
        base_port: int = BATTERY_BASE_PORT,
        build_profile: str = "release",
        fresh: bool = False,
    ) -> BatteryConfig:
        """Build from the environment plus the battery's CLI context."""
        images = DEFAULT_IMAGES if images is None else int(images)
        if images <= 200:
            repeats_default = 5
            timeout_default = 1800
        else:
            repeats_default = 3
            timeout_default = 7200
        repeats = integer(environ, "BATTERY_REPEATS", repeats_default, minimum=BATTERY_REPEATS_MIN)
        min_probe_rounds = integer(environ, "MIN_PROBE_ROUNDS", DEFAULT_MIN_PROBE_ROUNDS, minimum=1)
        if "WORKLOAD_TIMEOUT_S" in environ:
            workload_timeout = environ["WORKLOAD_TIMEOUT_S"]
        else:
            workload_timeout = str(timeout_default)
        return cls(
            images=images,
            out=Path(out),
            sets_root=Path(sets_root),
            base_port=base_port,
            build_profile=build_profile,
            fresh=fresh,
            repeats=repeats,
            min_probe_rounds=min_probe_rounds,
            workload_timeout_s=workload_timeout,
        )


# --- prepare layer -----------------------------------------------------------
def _prepare_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Prepare a benchmark sandbox from a canonical (read-only) archive."
    )
    parser.add_argument("--archive", required=True, help="canonical read-only archive root")
    parser.add_argument("--sandbox", required=True, help="target sandbox directory")
    parser.add_argument("--count", type=int, default=100, help="images to link (default 100)")
    parser.add_argument(
        "--large", action="store_true", help="large-file class archive (used as-is)"
    )
    parser.add_argument(
        "--force", action="store_true", help="allow non-empty sandbox (wiped first)"
    )
    return parser


@dataclass(frozen=True)
class PrepareConfig:
    """One `bench prepare` invocation."""

    archive: Path
    sandbox: Path
    count: int
    large: bool
    force: bool

    @classmethod
    def from_args(cls, argv=None) -> PrepareConfig:
        args = _prepare_parser().parse_args(argv)
        archive = Path(args.archive)
        if not archive.is_dir():
            raise ConfigError(f"archive is not a directory: {args.archive}")
        if isinstance(args.count, bool) or not isinstance(args.count, int) or args.count < 1:
            raise ConfigError(f"count must be a positive integer, got {args.count!r}")
        sandbox = Path(args.sandbox)
        if sandbox.exists() and archive.resolve() == sandbox.resolve():
            raise ConfigError("sandbox must differ from archive")
        return cls(
            archive=archive,
            sandbox=sandbox,
            count=args.count,
            large=args.large,
            force=args.force,
        )


# --- preflight layer ---------------------------------------------------------
@dataclass(frozen=True)
class PreflightConfig:
    """The inputs one preflight checklist is built from.

    Preflight reports instead of raising: each rule turns its input into a
    `Check`, so this object only normalises flags over the environment.
    """

    out: object
    fresh: bool
    port: object
    archive: object
    sandbox: object
    binary: object
    environ: dict

    @classmethod
    def from_env(
        cls,
        environ: dict,
        *,
        out=None,
        fresh=None,
        port=None,
        archive=None,
        sandbox=None,
        binary=None,
    ) -> PreflightConfig:
        return cls(
            out=out if out is not None else environ.get("OUT"),
            fresh=fresh if fresh is not None else environ.get("FRESH", "1") == "1",
            port=port if port is not None else environ.get("PORT", DEFAULT_PORT),
            archive=archive,
            sandbox=sandbox if sandbox is not None else environ.get("IMAGE_HOME_SRC"),
            binary=binary,
            environ=environ,
        )
