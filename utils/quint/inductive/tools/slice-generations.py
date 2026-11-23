#!/usr/bin/env python3
"""Codemod for Phase 1 slice 7b: generations epoch tokens with satSucc.

The core's `generations` is already a total map PATHS -> int (init writes every
path; DbLoss resets every path; no removal sites), so this slice only bounds the
value range: one saturation helper (`satSucc`) over one bound val, and every
`generations.get(X) + 1` bump replaced by `satSucc(generations.get(X))`.

The bound is `pure val GEN_LIMIT = 7`, not the plan's `const G` / `pure val G`:
both of those spellings collide with the proof entry, which declares its own
`pure val G` and imports this module with `.*`. A core constant would also
force every entry to bind it. `GEN_LIMIT` is entry-compatible (no entry binds
it, no entry declares it); the audit pins it to the proof entry's `G` value.
"""
import pathlib
import re
import sys

CORE = pathlib.Path(__file__).resolve().parents[1] / "inductive-core.qnt"

ANCHOR = "  pure def rootOf(p: str): str = if (p.in(SECOND_ROOT_PATHS)) \"r2\" else \"r1\"\n"
BOUND_DEF = (
    "\n"
    "  // Generation-token saturation bound for the epoch redesign\n"
    "  // (generations: PATHS -> 0..GEN_LIMIT). The bound lives in the core so\n"
    "  // satSucc is total; the proof entry's `G` must hold the same value (the\n"
    "  // audit pins the two).\n"
    "  pure val GEN_LIMIT = 7\n"
    "\n"
    "  // Epoch-token successor: monotone and saturating at GEN_LIMIT, so a bump\n"
    "  // never leaves the finite generation domain.\n"
    "  pure def satSucc(g: int): int = if (g < GEN_LIMIT) g + 1 else g\n"
)

NEEDLE = "generations.get("


def read_balanced(text, open_idx):
    assert text[open_idx] == "(", "expected '(' at %d" % open_idx
    depth = 0
    i = open_idx
    while i < len(text):
        c = text[i]
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


def rewrite_bumps(text):
    """Replace `generations.get(X) + 1` with `satSucc(generations.get(X))`."""
    count = 0
    out = []
    i = 0
    while True:
        idx = text.find(NEEDLE, i)
        if idx == -1:
            out.append(text[i:])
            break
        out.append(text[i:idx])
        open_idx = idx + len(NEEDLE) - 1
        close_idx = read_balanced(text, open_idx)
        if close_idx == -1:
            out.append(text[idx:])
            i = len(text)
            break
        end = close_idx + 1
        if text.startswith(" + 1", end):
            arg = text[open_idx + 1:close_idx]
            out.append("satSucc(generations.get(" + arg + "))")
            count += 1
            i = end + len(" + 1")
        else:
            out.append(text[idx:end])
            i = end
    return "".join(out), count


def main():
    text = CORE.read_text()
    original = text
    counts = {}

    # 0. One bound val and one helper (idempotent).
    if "pure def satSucc(g: int): int" not in text:
        assert ANCHOR in text, "rootOf anchor not found"
        text = text.replace(ANCHOR, ANCHOR + BOUND_DEF, 1)
        counts["satSucc_def"] = 1
    else:
        counts["satSucc_def"] = 0

    # 1. Every bump site, including the generation fields of newly built
    #    intake items (they carry the same post-bump token).
    text, c = rewrite_bumps(text)
    counts["bumps"] = c

    CORE.write_text(text)
    changed = text != original
    residual = len(re.findall(r"generations\.get\([^()]*\) \+ 1", text))
    print("slice-generations: counts = %s" % counts)
    print("slice-generations: file changed = %s" % changed)
    print("slice-generations: residual `generations.get(X) + 1` = %d" % residual)
    return 0


if __name__ == "__main__":
    sys.exit(main())
