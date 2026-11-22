#!/usr/bin/env python3
"""Codemod for Phase 1 slice 6: dupeIndex sparse key view -> total map over HASHES.

The source table is keyed by content hash and its key domain is constant:
`init` and `DbLoss` write every `h in HASHES`, later writes only `put` keys,
and no action removes a key.  The map is therefore total over `HASHES` in
every reachable state, with values `Set[int]` (subsets of IDS).

Key existence is consequently `h.in(HASHES)`, not `get(h) != Set()`: the
source stores empty buckets (`init` for `h != 0`, `DbLoss`, and any
`exclude` that empties a bucket), so presence and non-emptiness differ.
All three presence forms below are rewritten to the domain test, which also
guarantees the guarded `get(h)` stays inside the total domain.

Sites (census: 155 lines / 312 occurrences, 10 `keys()` occurrences):
  - 9 `dupeIndex.keys().contains(<hash>)` -> `(<hash>).in(HASHES)`
  - 1 `dupeIndex.keys().forall(h => ...)` -> `HASHES.forall(h => ...)`
  - init and DbLoss already write `HASHES.mapBy(...)`; left unchanged.

Re-runnable: a second run rewrites nothing and leaves the file identical.
"""
import pathlib
import sys

CORE = pathlib.Path(__file__).resolve().parents[1] / "inductive-core.qnt"

INIT_INITIALIZER = (
    "dupeIndex' = HASHES.mapBy(h => PATHS.filter(p => initialFile(p).assetHash == h)"
    ".map(p => initialId(p)))"
)
DBLOSS_INITIALIZER = "dupeIndex' = HASHES.mapBy(h => Set())"


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


def rewrite_presence(text):
    """`dupeIndex.keys().contains(X)` -> `(X).in(HASHES)`, nested args balanced."""
    needle = "dupeIndex.keys().contains("
    count = 0
    out = []
    i = 0
    while True:
        idx = text.find(needle, i)
        if idx == -1:
            out.append(text[i:])
            break
        out.append(text[i:idx])
        open_idx = idx + len(needle) - 1
        close_idx = read_balanced(text, open_idx)
        if close_idx == -1:
            out.append(text[idx:])
            i = len(text)
            break
        arg = text[open_idx + 1:close_idx]
        out.append("(" + arg + ").in(HASHES)")
        count += 1
        i = close_idx + 1
    return "".join(out), count


def main():
    text = CORE.read_text()
    original = text
    counts = {}

    # 1. Initializers stay total over HASHES; assert they are already the
    #    expected form so a silently sparse initializer cannot slip through.
    counts["init_total"] = text.count(INIT_INITIALIZER)
    counts["dbloss_total"] = text.count(DBLOSS_INITIALIZER)
    if counts["init_total"] != 1 or counts["dbloss_total"] != 1:
        print(
            "slice-dupeindex: expected one total init and one total DbLoss "
            "initializer, found %d and %d"
            % (counts["init_total"], counts["dbloss_total"]),
            file=sys.stderr,
        )
        return 1

    # 2. Presence tests: key existence is membership in the constant domain.
    text, counts["presence"] = rewrite_presence(text)

    # 3. Iteration: keys() over a total map is the constant domain.
    old_forall = "dupeIndex.keys().forall("
    counts["forall"] = text.count(old_forall)
    if counts["forall"]:
        text = text.replace(old_forall, "HASHES.forall(")

    # 4. No sparse key view survives.
    remaining = text.count("dupeIndex.keys()")
    if remaining:
        print(
            "slice-dupeindex: %d `dupeIndex.keys()` site(s) remain" % remaining,
            file=sys.stderr,
        )
        return 1

    CORE.write_text(text)
    changed = text != original
    print("slice-dupeindex: counts = %s" % counts)
    print("slice-dupeindex: file changed = %s" % changed)
    return 0


if __name__ == "__main__":
    sys.exit(main())
