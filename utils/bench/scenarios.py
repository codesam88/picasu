"""Cell specs: the benchmark matrix as data.

A cell is an ordered list of phases around an optional populate scan. The
matrix is (background x foreground); this module is the single source of
truth for which profiles exist and what their phases are.
"""

from __future__ import annotations

from dataclasses import dataclass

PROBE_FOREGROUNDS = ("prefetch", "edit", "thumb", "scroll")
REPORT_ACCESSES = ("prefetch", "edit", "thumb", "scroll_bar", "scroll_rows", "scroll_data")
STORM_KINDS = ("read", "thumb", "edit")
STORM_LEVELS = ("8", "16n")
STORM_ACCESS = {"read": "prefetch", "thumb": "thumb", "edit": "edit"}
ACCESS_LABEL = {
    "prefetch": "read",
    "edit": "edit",
    "thumb": "thumb",
    "scroll_bar": "scroll-bar",
    "scroll_rows": "scroll-rows",
    "scroll_data": "scroll-data",
}
ACCESS_ORDER = {subject: index for index, subject in enumerate(REPORT_ACCESSES)}

@dataclass(frozen=True)
class Phase:
    background: str
    foregrounds: tuple
    clients: int = 1
    issue: str = "rounds"  # rounds | stream | files
    stop: str = "scan"  # scan | samples | ops | files
    target: int = 0
    setup_scan: bool = False


@dataclass(frozen=True)
class CellSpec:
    profile: str
    phases: tuple
    probe_round_target: int = 0


def level_clients(level: str, ncpu: int) -> int:
    if level == "8":
        return 8
    if level == "16n":
        return 16 * max(1, int(ncpu))
    raise ValueError(f"unknown storm level: {level}")


def probe_label(subject: str) -> str:
    return f"probe-{ACCESS_LABEL[subject]}"


def storm_label(kind: str, level: str) -> str:
    return f"storm-{kind}-{level}"


def upload_label(phase: str) -> str:
    return f"upload-{phase}"


NO_LOAD_LABEL = "no-load"
UPLOAD_LOOP_LABEL = "upload-loop"


def storm_profile(background: str, kind: str, level: str) -> str:
    return f"{background}-storm-{kind}-{level}"


def upload_profile(background: str) -> str:
    return f"{background}-upload"


def upload_background(profile: str) -> str:
    """The background an upload cell measures against."""
    return "scan" if profile == upload_profile("scan") else "idle"


def storm_profiles() -> tuple:
    return tuple(
        storm_profile(background, kind, level)
        for background in ("scan", "idle")
        for kind in STORM_KINDS
        for level in STORM_LEVELS
    )


PROBE = "probe"
IDLE_UPLOAD = upload_profile("idle")
SCAN_UPLOAD = upload_profile("scan")


def profiles() -> tuple:
    return (PROBE, *storm_profiles(), IDLE_UPLOAD, SCAN_UPLOAD)


def is_storm(profile: str) -> bool:
    return profile.startswith(("scan-storm-", "idle-storm-"))


def storm_parts(profile: str) -> tuple:
    """(background, kind, level) for a storm profile."""
    if not is_storm(profile):
        raise ValueError(f"not a storm profile: {profile}")
    background, _, kind, level = profile.split("-", 3)
    return background, kind, level


def build_spec(
    profile: str,
    *,
    min_probe_rounds: int,
    repeats: int,
    upload_files: int,
    storm_ops: int,
    ncpu: int,
) -> CellSpec:
    """The phases a profile runs. This is the only profile dispatch."""
    if profile == PROBE:
        return CellSpec(
            profile=PROBE,
            phases=(
                Phase("scan", PROBE_FOREGROUNDS, issue="rounds", stop="scan"),
                Phase("idle", PROBE_FOREGROUNDS, issue="rounds", stop="samples", target=repeats),
            ),
            probe_round_target=min_probe_rounds,
        )
    if profile == IDLE_UPLOAD:
        return CellSpec(
            profile=profile,
            phases=(Phase("idle", ("upload",), issue="files", stop="files", target=upload_files),),
        )
    if profile == SCAN_UPLOAD:
        return CellSpec(
            profile=profile,
            phases=(Phase("scan", ("upload",), issue="files", stop="files", target=upload_files),),
        )
    if not is_storm(profile):
        raise ValueError(f"unknown profile: {profile}")
    background, kind, level = storm_parts(profile)
    clients = level_clients(level, ncpu)
    if background == "scan":
        phase = Phase(
            "scan",
            (STORM_ACCESS[kind],),
            clients=clients,
            issue="stream",
            stop="scan",
        )
    else:
        phase = Phase(
            "idle",
            (STORM_ACCESS[kind],),
            clients=clients,
            issue="stream",
            stop="ops",
            target=clients * storm_ops,
            setup_scan=True,
        )
    return CellSpec(profile=profile, phases=(phase,))
