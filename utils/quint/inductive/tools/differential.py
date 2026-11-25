#!/usr/bin/env python3
"""Paired forward-simulation differential between the source model and the
redesigned inductive core (plan .plan/quint-inductive-proof.md section 4.5).

The core (`inductive/inductive-core.qnt`) is a hand-and-codemod rewrite of
`pipeline/merged.qnt` into total maps with sentinels, epoch tokens and the
smallest-free-id quotient.  The rewrite must not change behavior, so both
models are sampled with the same seeds and sample counts and the resulting
traces are compared step by step:

  1. the action-name sequence (`mbt::actionTaken` in every ITF state);
  2. a goal-relevant projection of every state -- per-path row/record/metadata
     presence and content, filesystem record, generations, intake items,
     sweep/lease/unprocessable tables, phase machine variables, and the
     `Converged` / `AspectsAligned` goal predicates.

Generation tokens are compared through the redesigned model's saturation
quotient: the core stops a path's token at `GEN_LIMIT` while the source
model's counter keeps growing, so tokens are projected to `min(token,
GEN_LIMIT)` and the intake set is compared as the image of that projection (two
source items whose tokens straddle the bound coalesce, as they do in the core's
`Set[IntakeItem]`).

Saturation also changes the *model's* behavior (extra work, coalesced intake
items, a different enabled set), and the simulator draws all samples of one
invocation from a shared seed stream, so a saturated sample shifts every later
sample's trace.  The harness therefore treats a seed batch in which any sample
exceeds the bound as out of the strict-comparison regime: it reports the count
and the first difference, and does not fail the gate on it.  The default
horizon (`--max-steps 10`) is short enough that saturation does not occur:
100 samples per seed at seeds 1..4 produced zero tokens above 7, while the
40-step horizon saturates about half the samples.  Divergences in an
in-regime batch are behavioral deltas and fail the gate.

Each side's projection is derived from that side's own encoding (the core's
`assetByPath` presence is `get(p) != 0`, the source's is `keys().contains(p)`,
and so on) and only the decoded results are compared.  Asset ids themselves
are deliberately not compared: the core replaces the `nextAssetId` counter
with the smallest-free-id quotient (SCOPE-17), so id *values* may legitimately
differ while path-level row content agrees.  The harness counts and reports
those id-only differences as information.

Usage
-----
    utils/quint/inductive/tools/differential.py
    utils/quint/inductive/tools/differential.py --seeds 20 --samples 100
    utils/quint/inductive/tools/differential.py --core /tmp/mutant-core.qnt

To exercise the deeper action chains (faults, sweep batch apply) without
entering the saturated regime, copy the core to /tmp with a raised
`GEN_LIMIT`, pass `--core <copy> --epoch-cap <bound> --max-steps 40`, and read
the result as rewrite evidence below the bound (the executed `satSucc` path is
the same as long as no token reaches the bound).

Exit status: 0 when every compared trace is identical, 1 on a divergence in a
paired (in-regime) seed batch (printed as side, seed, trace, step, action,
field, merged value, core value), 2 on a toolchain/usage problem (quint
missing, typecheck failure, run error on one side only), 3 when the
wall-clock cap stopped the run before completion.

Flag spellings were checked against quint 0.33.1 (`quint run --help`).  Notes
that shaped the invocation:

  * `--invariant` (singular) takes one definition name; `--invariants` is the
    space-separated array form.  The harness uses the singular form.
  * `--mbt` writes `mbt::actionTaken` / `mbt::nondetPicks` into the ITF state.
  * With a fixed `--seed`, `--n-traces=K --max-samples=K` produces K sampled
    traces.  Trace-to-`{seq}` assignment depends on the shared simulator RNG,
    so the two sides stay index-aligned only when the sample count and the
    thread count are identical; `--n-threads=1` pins the ordering (sample
    file `{seq}` is stable across repeated invocations).
  * `--init init --step step` leaves `phase` unset, so `CompletionInvariant`
    cannot be checked in that mode (`QNT502`); the pipeline mode therefore
    runs without an invariant.  The default mode is the completion machine
    (`initCompletion` / `stepCompletion`), which includes every pipeline
    action plus the phase-1/2 actions.

The sampled instance binds all five path names the model's action
enumerations hardcode (`p1..p5`), reduced `HASHES = 0..1` and `SMALL = 0..2`,
`SECOND_ROOT_PATHS = {"p5"}`, one subtree pair.  The three-path "reduced"
instance cannot be sampled: environment actions evaluate their `nondet`
preambles before the `p.in(PATHS)` guard, so a branch on `p4`/`p5` dies with
`QNT507` (non-existing map key) in both models.  All three-path entries in the
lane are typecheck/test entries, not `quint run` entries.

The model files are copied into the scratch tree and imported from there, so a
concurrent edit cannot mix versions across the seed loop.  The scratch
`quint typecheck` of both entries is the plan's typecheck gate (section 2.4).
Toolchain caveat: quint 0.33.1 does not report a syntax error *inside an
imported module* when typechecking the importer -- only `quint parse <model>`
does.  The lane's entry-based typecheck gate is blind to model-level parse
errors; the models are nevertheless parsed effectively the same in both lanes.

The goal predicates are re-implemented here over the decoded abstract tables
(one port of the source model's `Converged` / `AspectAligned` definitions,
applied to both sides' states), because Quint does not emit derived values
into the ITF trace.  Consequently a mutation *inside* the predicate text is
not reached by this harness; mutations of the state writes the predicates read
are.  The suite's assertions and `audit.py` cover the definitions themselves.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

# ---------------------------------------------------------------------------
# Instance and comparison constants
# ---------------------------------------------------------------------------

PATHS = ("p1", "p2", "p3", "p4", "p5")
ROOTS = ("r1", "r2")
HASHES = (0, 1)

ENTRY_TEMPLATE = """\
// Scratch differential entry. Generated by tools/differential.py; the two
// entries differ only in the import path.
module {module} {{
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3", "p4", "p5"),
    HASHES = 0.to(1),
    SMALL = 0.to(2),
    SECOND_ROOT_PATHS = Set("p5"),
    SUBTREE_PAIRS = [{{ src: "p1", dst: "p3" }}],
  ).* from "{import_path}"
}}
"""

SIDES = ("merged", "core")
SIDE_MODEL = {
    "merged": ("./pipeline/merged", "pipeline/merged.qnt"),
    "core": ("./inductive/inductive-core", "inductive/inductive-core.qnt"),
}
SIDE_MODULE = {"merged": "diff_entry_merged", "core": "diff_entry_core"}

STATE_VARS = (
    "files",
    "generations",
    "intake",
    "assetById",
    "assetByPath",
    "dupeIndex",
    "metadataPayload",
    "thumbPresent",
    "sweepPending",
    "sweepBatch",
    "leasedPaths",
    "unprocessable",
)
PHASE_VARS = ("phase", "scannedRoots", "faults")


# ---------------------------------------------------------------------------
# ITF decoding
# ---------------------------------------------------------------------------


def decode(value):
    """Decode one ITF value into plain Python (idempotent on plain values)."""
    if isinstance(value, dict):
        if set(value) == {"#bigint"}:
            return int(value["#bigint"])
        if set(value) == {"#set"}:
            return [decode(v) for v in value["#set"]]
        if set(value) == {"#tup"}:
            return tuple(decode(v) for v in value["#tup"])
        if set(value) == {"#list"}:
            return [decode(v) for v in value["#list"]]
        if set(value) == {"#map"}:
            return {decode(k): decode(v) for k, v in value["#map"]}
        return {k: decode(v) for k, v in value.items()}
    if isinstance(value, list):
        return [decode(v) for v in value]
    return value


def canon(value) -> str:
    """Canonical JSON text for set-like comparisons (order-insensitive)."""
    if isinstance(value, list):
        return json.dumps(sorted(canon(v) for v in value))
    return json.dumps(value, sort_keys=True, default=str)


def state_vars(state: dict) -> dict:
    """Strip the import-instance qualifier from the ITF state variable names."""
    out = {}
    for name, value in state.items():
        if name.startswith("mbt::"):
            continue
        out[name.rsplit("::", 1)[-1]] = value
    return out


# ---------------------------------------------------------------------------
# Per-state projection
# ---------------------------------------------------------------------------


class Projection:
    """Goal-relevant view of one state, decoded from that side's own encoding.

    `as_leaves()` returns an ordered dict of `field -> JSON-comparable value`;
    the ordering is the comparison order (per-path rows first, then the shared
    tables, then the goal predicates), so the first reported difference is the
    most specific one.

    Epoch tokens (`generations`, the generation field of intake items, the
    `sweepBatch` values) are compared through the saturation quotient: the
    core's tokens stop at `GEN_LIMIT` while the source model's counter keeps
    growing, so the observable is `min(token, GEN_LIMIT)` (SCOPE-18).  A token
    table that saturates at the wrong bound still shows.
    """

    def __init__(
        self, side: str, state: dict, step: int, epoch_cap: int, mode: str = "completion"
    ):
        self.side = side
        self.step = step
        self.epoch_cap = epoch_cap
        vars_ = {k: decode(v) for k, v in state_vars(state).items()}
        required = STATE_VARS + (
            PHASE_VARS if mode == "completion" else ()
        )
        missing = [v for v in required if v not in vars_]
        if missing:
            raise HarnessError(
                f"{side} trace step {step}: missing state variables {missing}"
            )
        self.vars = vars_
        self.files = vars_["files"]
        self.generations = vars_["generations"]
        self.asset_by_id = vars_["assetById"]
        self.asset_by_path = vars_["assetByPath"]
        self.metadata = vars_["metadataPayload"]
        self.thumb = vars_["thumbPresent"]
        self.dupe = vars_["dupeIndex"]
        self.intake = vars_["intake"]
        self.sweep_pending = vars_["sweepPending"]
        self.sweep_batch = vars_["sweepBatch"]
        self.leased = vars_["leasedPaths"]
        self.unprocessable = vars_["unprocessable"]
        self.phase = vars_.get("phase")
        self.scanned_roots = vars_.get("scannedRoots")
        self.faults = vars_.get("faults")

        if side == "merged":
            # Source encoding: presence is key membership in a sparse map.
            self.row_id = dict(self.asset_by_path)
            self.asset_present_ids = set(self.asset_by_id)
            self.meta_present_ids = set(self.metadata)
        elif side == "core":
            # Redesigned encoding: total maps, absence is the sentinel value.
            self.row_id = {
                p: i for p, i in self.asset_by_path.items() if i != 0
            }
            self.asset_present_ids = {
                i for i, r in self.asset_by_id.items() if r["kind"] != "absent"
            }
            self.meta_present_ids = {
                i for i, r in self.metadata.items() if r["kind"] != "absent"
            }
        else:  # pragma: no cover - guarded by SIDES
            raise HarnessError(f"unknown side {side!r}")

    # -- presence helpers ---------------------------------------------------

    def row_present(self, p: str) -> bool:
        return p in self.row_id

    def asset_present(self, i: int) -> bool:
        return i in self.asset_present_ids

    def meta_present(self, i: int) -> bool:
        return i in self.meta_present_ids

    def thumb_present(self, h: int) -> bool:
        return bool(self.thumb.get(h, False))

    def dupe_has(self, h: int, i: int) -> bool:
        ids = self.dupe.get(h)
        return ids is not None and i in ids

    # -- derived goal predicates (port of the source definitions) -----------

    def converged(self) -> bool:
        for p in PATHS:
            if p in self.unprocessable:
                continue
            f = self.files[p]
            if bool(f["present"]) != self.row_present(p):
                return False
            if not f["present"]:
                continue
            aid = self.row_id[p]
            if not self.asset_present(aid):
                return False
            a = self.asset_by_id[aid]
            if a["path"] != p or a["contentHash"] != f["ver"]:
                return False
            if not self.meta_present(aid):
                return False
            m = self.metadata[aid]
            if (
                m["ver"] != f["ver"]
                or m["fileSize"] != f["len"]
                or not m["converged"]
            ):
                return False
            if not (a["contentHash"] < 0 or self.thumb_present(a["contentHash"])):
                return False
        return len(self.intake) == 0

    def aspects_aligned(self) -> bool:
        for p in PATHS:
            if p in self.unprocessable:
                continue
            f = self.files[p]
            if f["present"]:
                if not self.row_present(p):
                    return False
                aid = self.row_id[p]
                if not self.asset_present(aid):
                    return False
                a = self.asset_by_id[aid]
                if a["path"] != p or a["contentHash"] != f["ver"]:
                    return False
                if not self.meta_present(aid):
                    return False
                m = self.metadata[aid]
                if (
                    m["ver"] != f["ver"]
                    or m["fileSize"] != f["len"]
                    or not m["converged"]
                ):
                    return False
                if not (
                    a["contentHash"] < 0 or self.thumb_present(a["contentHash"])
                ):
                    return False
                if not (a["contentHash"] < 0 or self.dupe_has(a["contentHash"], aid)):
                    return False
            elif self.row_present(p):
                return False
        return True

    # -- id-only information (not a failure) --------------------------------

    def ids(self) -> dict:
        """Id values and the allocation cursor, for the quotient note."""
        info = {"rows": dict(self.row_id)}
        if self.side == "merged":
            info["cursor"] = self.vars.get("nextAssetId")
        else:
            # Smallest free media id, mirroring `def nextFreeId` in the core.
            cursor = len(PATHS) + 2
            for i in range(1, len(PATHS) + 2):
                if i not in self.asset_present_ids and i < cursor:
                    cursor = i
            info["cursor"] = cursor
        return info

    # -- comparison view ----------------------------------------------------

    def as_leaves(self) -> dict:
        out = {}

        for p in PATHS:
            f = self.files[p]
            for field in ("present", "ver", "len"):
                out[f"files.{p}.{field}"] = f[field]
            out[f"rows.{p}.present"] = self.row_present(p)
            if self.row_present(p):
                aid = self.row_id[p]
                a = self.asset_by_id.get(aid) if self.asset_present(aid) else None
                m = self.metadata.get(aid) if self.meta_present(aid) else None
                out[f"rows.{p}.asset.present"] = a is not None
                if a is not None:
                    for field in (
                        "kind",
                        "path",
                        "contentHash",
                        "albumId",
                        "isTrashed",
                    ):
                        out[f"rows.{p}.asset.{field}"] = a[field]
                out[f"rows.{p}.metadata.present"] = m is not None
                if m is not None:
                    for field in (
                        "kind",
                        "ver",
                        "fileSize",
                        "converged",
                        "itemCount",
                        "itemSize",
                        "hasShareList",
                    ):
                        out[f"rows.{p}.metadata.{field}"] = m[field]
                out[f"rows.{p}.thumb"] = (
                    a is not None
                    and a["contentHash"] >= 0
                    and self.thumb_present(a["contentHash"])
                )
            out[f"generations.{p}"] = self._epoch(self.generations.get(p))

        for h in HASHES:
            out[f"thumbPresent.{h}"] = self.thumb_present(h)
            out[f"dupeIndex.{h}"] = canon(self._dupe_paths(h))

        for r in ROOTS:
            out[f"sweepPending.{r}"] = bool(self.sweep_pending.get(r, False))
        for p in PATHS:
            v = self.sweep_batch.get(p)
            if v is not None and v != -1:
                out[f"sweepBatch.{p}"] = self._epoch(v)

        intake_items = sorted(
            {canon(self._intake_view(i)) for i in self.intake}
        )
        out["intake"] = json.dumps(intake_items)
        out["intake.size"] = len(intake_items)
        out["leasedPaths"] = canon(self.leased)
        out["unprocessable"] = canon(self.unprocessable)
        out["scannedRoots"] = canon(self.scanned_roots)
        out["phase"] = self.phase
        out["faults"] = self.faults

        out["records"] = canon(self._present_records())
        out["metadataRecords"] = canon(self._present_metadata())

        out["goal.Converged"] = self.converged()
        out["goal.AspectsAligned"] = self.aspects_aligned()
        return out

    # -- epoch-token quotient ----------------------------------------------

    def _epoch(self, token):
        if token is None:
            return None
        return min(token, self.epoch_cap)

    def _intake_view(self, item: dict) -> dict:
        return {
            k: (self._epoch(v) if k == "generation" else v)
            for k, v in item.items()
        }

    def _present_records(self) -> list:
        return [
            {k: v for k, v in self.asset_by_id[i].items() if k != "assetId"}
            for i in sorted(self.asset_present_ids)
        ]

    def _present_metadata(self) -> list:
        return [self.metadata[i] for i in sorted(self.meta_present_ids)]

    def _dupe_paths(self, h: int) -> list:
        out = []
        for i in self.dupe.get(h, []):
            if self.asset_present(i) and self.asset_by_id[i]["path"]:
                out.append(self.asset_by_id[i]["path"])
            else:
                out.append("<unresolved>")
        return out


# ---------------------------------------------------------------------------
# Harness plumbing
# ---------------------------------------------------------------------------


class HarnessError(Exception):
    pass


def log(msg: str) -> None:
    print(msg, flush=True)


def run(cmd, timeout, cwd=None):
    return subprocess.run(
        cmd,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=timeout,
    )


def write_entries(workdir: str, merged_path: str, core_path: str) -> tuple:
    """Create the scratch directory tree and the two entries.

    The entries are generated from one template and differ only in the import
    path.  The models are *copied* under those paths (not symlinked), so a
    concurrent edit cannot mix model versions across the seed loop; the copies'
    digests are recorded and re-checked against the live files at the end.
    """
    os.makedirs(os.path.join(workdir, "pipeline"), exist_ok=True)
    os.makedirs(os.path.join(workdir, "inductive"), exist_ok=True)
    os.makedirs(os.path.join(workdir, "logs"), exist_ok=True)

    copies = {
        os.path.join(workdir, "pipeline", "merged.qnt"): merged_path,
        os.path.join(workdir, "inductive", "inductive-core.qnt"): core_path,
    }
    for dest, source in copies.items():
        if os.path.islink(dest):
            os.remove(dest)  # never write through a stale symlink into the repo
        shutil.copyfile(source, dest)

    entries = {}
    for side in SIDES:
        import_path, _ = SIDE_MODEL[side]
        text = ENTRY_TEMPLATE.format(module=SIDE_MODULE[side], import_path=import_path)
        path = os.path.join(workdir, f"entry-{side}.qnt")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(text)
        entries[side] = path

    # The two entries must be identical except for the import path (and the
    # module name derived from it).
    body = {
        side: [
            line
            for line in open(entries[side], encoding="utf-8")
            if "from " not in line and "module " not in line
        ]
        for side in SIDES
    }
    if body["merged"] != body["core"]:
        raise HarnessError("generated entries differ outside the import path")
    if os.path.realpath(merged_path) == os.path.realpath(core_path):
        raise HarnessError("both entries resolve to the same model file")

    return entries, copies


def digest(path: str) -> str:
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def typecheck(entries: dict, timeout: int, retry_wait: int, workdir: str) -> None:
    for side in SIDES:
        log_path = os.path.join(workdir, "logs", f"typecheck-{side}.log")
        deadline = time.monotonic() + timeout
        attempts = 0
        while True:
            attempts += 1
            try:
                proc = run(
                    ["quint", "typecheck", entries[side]],
                    timeout=max(1, int(deadline - time.monotonic())),
                )
            except subprocess.TimeoutExpired:
                raise HarnessError(f"quint typecheck {side} timed out")
            with open(log_path, "w", encoding="utf-8") as handle:
                handle.write(proc.stdout)
            if proc.returncode == 0 and "QNT" not in proc.stdout:
                break
            if attempts == 2 or retry_wait <= 0:
                raise HarnessError(
                    f"quint typecheck failed for the {side} entry "
                    f"(log: {log_path})\n{tail(proc.stdout)}"
                )
            log(
                f"  typecheck of the {side} entry failed (concurrent edit?); "
                f"retrying once in {retry_wait}s: {log_path}"
            )
            time.sleep(retry_wait)

    # quint 0.33.1 does not surface syntax errors from imported modules in the
    # importer's typecheck; `quint parse` does.  Report it without failing the
    # lane's entry-based gate.
    for side, rel in (
        ("merged", ("pipeline", "merged.qnt")),
        ("core", ("inductive", "inductive-core.qnt")),
    ):
        model = os.path.join(workdir, *rel)
        try:
            proc = run(["quint", "parse", model], timeout=timeout)
        except subprocess.TimeoutExpired:
            continue
        if proc.returncode != 0:
            first = next(
                (line.strip() for line in proc.stdout.splitlines() if line.strip()),
                "parse error",
            )
            log(
                f"  note: `quint parse` reports a syntax problem in the {side} "
                f"model ({first}); the imported-module typecheck gate passed -- "
                "quint 0.33.1 does not report imported-module parse errors"
            )


def tail(text: str, limit: int = 40) -> str:
    lines = text.strip().splitlines()
    return "\n".join(lines[-limit:])


def check_epoch_cap(core_path: str, epoch_cap: int) -> None:
    """Keep the saturation quotient tied to the core's declared bound.

    The harness normalizes generation tokens by `min(token, epoch_cap)`; if the
    core's `GEN_LIMIT` moved (the plan's raise rule) without the harness being
    updated, the normalization would silently compare different quotients.
    """
    with open(core_path, encoding="utf-8") as handle:
        text = handle.read()
    match = re.search(r"pure\s+val\s+GEN_LIMIT\s*=\s*(\d+)", text)
    if match is None:
        raise HarnessError(
            "cannot find `pure val GEN_LIMIT = <int>` in the core; the differential "
            "needs the saturation bound to normalize generation tokens"
        )
    declared = int(match.group(1))
    if declared != epoch_cap:
        raise HarnessError(
            f"core declares GEN_LIMIT = {declared} but --epoch-cap is {epoch_cap}; "
            "pass --epoch-cap to match the core's saturation bound"
        )


def trace_paths(workdir: str, side: str, seed: int, samples: int) -> list:
    out = []
    for seq in range(samples):
        path = os.path.join(workdir, f"{side}-{seed}-{seq}.json")
        if os.path.exists(path):
            out.append(path)
    return out


def load_trace(path: str) -> dict:
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def actions(trace: dict) -> list:
    return [state.get("mbt::actionTaken") for state in trace["states"]]


def raw_generation_max(trace: dict) -> int:
    """Largest generation token in the trace, as stored by that side."""
    best = 0
    for state in trace["states"]:
        generations = decode(state_vars(state).get("generations", {}))
        for token in generations.values():
            if token > best:
                best = token
    return best


def first_divergence(
    merged: Projection, core: Projection
) -> tuple | None:
    a = merged.as_leaves()
    b = core.as_leaves()
    for field in a:
        if a[field] != b.get(field):
            return field, a[field], b.get(field)
    for field in b:
        if field not in a:
            return field, "<absent>", b[field]
    return None


def compare_traces(
    merged_trace: dict,
    core_trace: dict,
    seed: int,
    seq: int,
    stats: dict,
    divergences: list,
    epoch_cap: int,
    mode: str = "completion",
    max_report: int = 5,
) -> None:
    ma = actions(merged_trace)
    ca = actions(core_trace)
    stats["traces"] += 1
    stats["states"] += min(len(ma), len(ca))

    if merged_trace.get("#meta", {}).get("status") != core_trace.get(
        "#meta", {}
    ).get("status"):
        divergences.append(
            (
                seed,
                seq,
                0,
                ma[0] if ma else "<empty>",
                "trace.status",
                merged_trace.get("#meta", {}).get("status"),
                core_trace.get("#meta", {}).get("status"),
            )
        )

    limit = min(len(ma), len(ca))
    for step in range(limit):
        action = ma[step]
        if action != ca[step]:
            divergences.append(
                (seed, seq, step, action, "mbt::actionTaken", action, ca[step])
            )
            return
        merged_proj = Projection(
            "merged", merged_trace["states"][step], step, epoch_cap, mode
        )
        core_proj = Projection(
            "core", core_trace["states"][step], step, epoch_cap, mode
        )
        stats["states_compared"] += 1

        # Id-only information: same path rows, different id values is the
        # recorded SCOPE-17 quotient, not a divergence.
        mid = merged_proj.ids()
        cid = core_proj.ids()
        if mid["rows"] != cid["rows"] or mid["cursor"] != cid["cursor"]:
            stats["id_differences"] += 1

        diff = first_divergence(merged_proj, core_proj)
        if diff is not None:
            field, mval, cval = diff
            divergences.append((seed, seq, step, action, field, mval, cval))
            if len(divergences) >= max_report:
                stats["divergences_capped"] = True
            return

    if len(ma) != len(ca):
        divergences.append(
            (
                seed,
                seq,
                limit,
                ma[limit] if limit < len(ma) else ca[limit],
                "trace.length",
                len(ma),
                len(ca),
            )
        )


def sample_once(
    entry: str,
    main: str,
    side: str,
    seed: int,
    args,
    workdir: str,
) -> tuple:
    """Run one side for one seed; return (loaded traces, run status)."""
    out_pattern = os.path.join(workdir, f"{side}-{seed}-{{seq}}.json")
    cmd = [
        "quint",
        "run",
        entry,
        "--main",
        main,
        "--mbt",
        "--n-traces",
        str(args.samples),
        "--max-samples",
        str(args.samples),
        "--seed",
        str(seed),
        "--n-threads",
        "1",
        "--max-steps",
        str(args.max_steps),
        "--init",
        args.init,
        "--step",
        args.step,
        "--out-itf",
        out_pattern,
    ]
    if args.invariant:
        cmd += ["--invariant", args.invariant]

    log_path = os.path.join(workdir, "logs", f"{side}-{seed}.log")
    try:
        proc = run(cmd, timeout=args.run_timeout)
    except subprocess.TimeoutExpired:
        raise HarnessError(
            f"quint run timed out after {args.run_timeout}s for {side} seed {seed}"
        )
    with open(log_path, "w", encoding="utf-8") as handle:
        handle.write(proc.stdout)
    return proc.returncode, log_path


def main(argv=None) -> int:
    here = os.path.dirname(os.path.abspath(__file__))
    lane = os.path.dirname(here)
    repo = os.path.dirname(os.path.dirname(os.path.dirname(lane)))
    default_merged = os.path.join(repo, "utils", "quint", "pipeline", "merged.qnt")
    default_core = os.path.join(lane, "inductive-core.qnt")

    parser = argparse.ArgumentParser(
        description="Paired forward-simulation differential (plan section 4.5)."
    )
    parser.add_argument("--seeds", type=int, default=20, help="number of seeds")
    parser.add_argument("--seed0", type=int, default=1, help="first seed")
    parser.add_argument(
        "--samples",
        type=int,
        default=100,
        help="sampled traces per seed and side (n-traces = max-samples)",
    )
    parser.add_argument(
        "--max-steps",
        type=int,
        default=10,
        help="maximum steps per trace (default keeps generation tokens below "
        "GEN_LIMIT; raise only to probe the saturated regime)",
    )
    parser.add_argument(
        "--mode",
        choices=("completion", "pipeline"),
        default="completion",
        help="completion machine (default) or the bare pipeline step",
    )
    parser.add_argument(
        "--core",
        default=default_core,
        help="core model to compare (default: the lane's inductive-core.qnt)",
    )
    parser.add_argument(
        "--merged", default=default_merged, help="source model (default: merged.qnt)"
    )
    parser.add_argument(
        "--workdir",
        default=None,
        help="scratch directory (default: a fresh directory under /tmp/opencode/diff)",
    )
    parser.add_argument(
        "--keep-traces",
        action="store_true",
        help="keep the ITF traces after a passing run",
    )
    parser.add_argument(
        "--run-timeout", type=int, default=300, help="timeout per quint invocation"
    )
    parser.add_argument(
        "--wall-cap", type=int, default=1800, help="total wall-clock cap in seconds"
    )
    parser.add_argument(
        "--retry-wait",
        type=int,
        default=600,
        help="wait before retrying a failed typecheck (concurrent edit)",
    )
    parser.add_argument(
        "--epoch-cap",
        type=int,
        default=7,
        help="saturation bound for generation tokens (must match the core's GEN_LIMIT)",
    )
    parser.add_argument(
        "--max-report", type=int, default=5, help="stop after this many divergences"
    )
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args(argv)

    if args.mode == "completion":
        args.init, args.step, args.invariant = (
            "initCompletion",
            "stepCompletion",
            "CompletionInvariant",
        )
    else:
        args.init, args.step, args.invariant = "init", "step", None

    if shutil.which("quint") is None:
        log("error: quint not found on PATH")
        return 2

    workdir = args.workdir
    if workdir is None:
        base = "/tmp/opencode/diff"
        os.makedirs(base, exist_ok=True)
        workdir = tempfile.mkdtemp(dir=base, prefix="run-")
    else:
        os.makedirs(workdir, exist_ok=True)

    started = time.monotonic()
    seeds = list(range(args.seed0, args.seed0 + args.seeds))

    log("core<->merged differential (plan section 4.5)")
    log(f"  merged model: {os.path.abspath(args.merged)}")
    log(f"  core model:   {os.path.abspath(args.core)}")
    log(
        f"  instance: PATHS=p1..p5, HASHES=0..1, SMALL=0..2, "
        f"SECOND_ROOT_PATHS={{p5}}, SUBTREE_PAIRS=[p1->p3]"
    )
    log(
        f"  seeds {seeds[0]}..{seeds[-1]} ({len(seeds)}), samples/seed {args.samples}, "
        f"max-steps {args.max_steps}, mode {args.mode}"
    )
    log(f"  scratch dir: {workdir}")

    try:
        entries, copies = write_entries(workdir, args.merged, args.core)
        core_copy = copies[os.path.join(workdir, "inductive", "inductive-core.qnt")]
        merged_copy = copies[os.path.join(workdir, "pipeline", "merged.qnt")]
        check_epoch_cap(core_copy, args.epoch_cap)
        log(
            "  entries: "
            + "  ".join(os.path.basename(entries[s]) for s in SIDES)
            + " (identical except the import path)"
        )
        log(
            f"  model digests: merged {digest(merged_copy)[:16]}  "
            f"core {digest(core_copy)[:16]}"
            " (scratch copies; the import paths resolve to these)"
        )
        log(
            "  command template: quint run <entry> --main <module> --mbt "
            f"--n-traces={args.samples} --max-samples={args.samples} "
            "--seed=<seed> --n-threads=1 "
            f"--max-steps={args.max_steps} --init {args.init} --step {args.step}"
            + (f" --invariant {args.invariant}" if args.invariant else "")
            + " --out-itf=<side>-<seed>-{seq}.json"
        )
        typecheck(entries, args.run_timeout, args.retry_wait, workdir)
    except HarnessError as error:
        log(f"error: {error}")
        return 2

    snapshot = {
        "merged": digest(os.path.join(workdir, "pipeline", "merged.qnt")),
        "core": digest(os.path.join(workdir, "inductive", "inductive-core.qnt")),
    }
    stats = {
        "traces": 0,
        "states": 0,
        "states_compared": 0,
        "id_differences": 0,
        "violations": 0,
        "runs": 0,
        "seeds_out_of_regime": 0,
        "traces_out_of_regime": 0,
        "divergences_out_of_regime": 0,
    }
    divergences = []
    out_of_regime = []

    try:
        for seed in seeds:
            if time.monotonic() - started > args.wall_cap:
                log(
                    f"stopped: wall cap of {args.wall_cap}s reached before seed {seed}"
                )
                return 3
            traces = {}
            for side in SIDES:
                rc, log_path = sample_once(
                    entries[side],
                    SIDE_MODULE[side],
                    side,
                    seed,
                    args,
                    workdir,
                )
                stats["runs"] += 1
                paths = trace_paths(workdir, side, seed, args.samples)
                if rc not in (0, 1) or not paths:
                    details = tail(open(log_path, encoding="utf-8").read())
                    raise HarnessError(
                        f"quint run failed for {side} seed {seed} "
                        f"(exit {rc}, log: {log_path})\n{details}"
                    )
                loaded = [load_trace(path) for path in paths]
                for trace in loaded:
                    if trace.get("#meta", {}).get("status") != "ok":
                        stats["violations"] += 1
                traces[side] = loaded

            if len(traces["merged"]) != len(traces["core"]):
                divergences.append(
                    (
                        seed,
                        -1,
                        0,
                        "<trace-count>",
                        "trace.count",
                        len(traces["merged"]),
                        len(traces["core"]),
                    )
                )
                break
            if len(traces["merged"]) != args.samples:
                log(
                    f"  warning: seed {seed} produced {len(traces['merged'])} traces "
                    f"(requested {args.samples}); an invariant violation truncates "
                    "the sample set"
                )

            before = len(divergences)
            saturated = [
                seq
                for seq, trace in enumerate(traces["merged"])
                if raw_generation_max(trace) > args.epoch_cap
            ] + [
                seq
                for seq, trace in enumerate(traces["core"])
                if raw_generation_max(trace) > args.epoch_cap
            ]
            if saturated:
                # One saturated sample shifts the shared sample stream, so the
                # whole seed batch is outside the paired regime.
                stats["seeds_out_of_regime"] += 1
                stats["traces_out_of_regime"] += len(set(saturated))
                batch = []
                scratch_stats = {key: 0 for key in stats}
                for seq in range(len(traces["merged"])):
                    compare_traces(
                        traces["merged"][seq],
                        traces["core"][seq],
                        seed,
                        seq,
                        scratch_stats,
                        batch,
                        args.epoch_cap,
                        args.mode,
                        max_report=1,
                    )
                stats["divergences_out_of_regime"] += len(batch)
                out_of_regime.extend(batch)
                log(
                    f"  seed {seed}: {len(traces['merged'])} traces, "
                    f"OUT OF REGIME (generation token exceeds {args.epoch_cap} in "
                    f"{len(set(saturated))} sample(s); batch not paired)"
                )
                continue

            for seq in range(len(traces["merged"])):
                compare_traces(
                    traces["merged"][seq],
                    traces["core"][seq],
                    seed,
                    seq,
                    stats,
                    divergences,
                    args.epoch_cap,
                    args.mode,
                    args.max_report,
                )
                if len(divergences) >= args.max_report:
                    break
            if args.verbose or len(divergences) > before:
                log(
                    f"  seed {seed}: {len(traces['merged'])} traces, "
                    f"{'DIVERGENCE' if len(divergences) > before else 'identical'}"
                )
            else:
                log(f"  seed {seed}: {len(traces['merged'])} traces identical")
            if len(divergences) >= args.max_report:
                break
    except HarnessError as error:
        log(f"error: {error}")
        return 2

    for side, live in (("merged", args.merged), ("core", args.core)):
        if digest(live) != snapshot[side]:
            log(
                f"  warning: the {side} model changed on disk during the run; "
                f"results apply to the snapshot {snapshot[side][:16]}"
            )

    elapsed = time.monotonic() - started
    log("")
    if divergences:
        log(f"DIVERGENCE ({len(divergences)}):")
        for seed, seq, step, action, field, mval, cval in divergences:
            log(
                f"  seed={seed} trace={seq} step={step} action={action} "
                f"field={field}"
            )
            log(f"    merged: {mval}")
            log(f"    core:   {cval}")
        if stats.get("divergences_capped"):
            log(f"  (stopped at --max-report={args.max_report})")
        log("")
        log(
            f"RESULT: divergent in {elapsed:.1f}s "
            f"({stats['traces']} trace pairs, {stats['states_compared']} state pairs)"
        )
        return 1

    log(
        f"RESULT: no divergence across {stats['traces']} trace pairs "
        f"({stats['states_compared']} state pairs) in {elapsed:.1f}s"
    )
    log(
        f"  action sequences identical; projection identical in every compared "
        f"state; goal predicates computed and compared"
    )
    if stats["seeds_out_of_regime"]:
        log(
            f"  note: {stats['seeds_out_of_regime']} seed batch(es) were out of the "
            f"paired regime ({stats['traces_out_of_regime']} sample(s) exceeded "
            f"GEN_LIMIT={args.epoch_cap}); their seed stream is unpaired after that "
            f"point, {stats['divergences_out_of_regime']} difference(s) observed and "
            "not counted (epoch saturation; see the module docstring)"
        )
        for seed, seq, step, action, field, mval, cval in out_of_regime[:3]:
            log(
                f"    first: seed={seed} trace={seq} step={step} action={action} "
                f"field={field} (merged {mval} / core {cval})"
            )
    if stats["id_differences"]:
        log(
            f"  note: {stats['id_differences']} state pairs differ only in asset id "
            "values / allocation cursor (SCOPE-17 smallest-free-id quotient; not a "
            "divergence)"
        )
    if stats["violations"]:
        log(
            f"  note: {stats['violations']} trace(s) reported an invariant violation "
            "on both sides (compared as part of the trace)"
        )
    if args.keep_traces:
        log(f"  traces kept in {workdir}")
    else:
        for name in sorted(os.listdir(workdir)):
            if name.endswith(".json"):
                os.remove(os.path.join(workdir, name))
        log(f"  traces removed; entries and logs kept in {workdir}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(130)
