#!/usr/bin/env python3
"""Codemod for Phase 1 slice 3: assetByPath sparse -> total map PATHS -> int (sentinel 0)."""
import pathlib
import sys

CORE = pathlib.Path(__file__).resolve().parents[1] / "inductive-core.qnt"


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


def count_replace(text, old, new):
    c = text.count(old)
    if c:
        text = text.replace(old, new)
    return text, c


def rewrite_presence(text):
    needle = "assetByPath.keys().contains("
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
        out.append("(assetByPath.get(" + arg + ") != 0)")
        count += 1
        i = close_idx + 1
    return "".join(out), count


def main():
    text = CORE.read_text()
    original = text
    counts = {}

    # 1. DbLoss initializer Map() -> total all-zero.
    text, c = count_replace(text, "    assetByPath' = Map(),\n", "    assetByPath' = PATHS.mapBy(p => 0),\n")
    counts["dbloss_init"] = c

    # 2. rows projection: keep only present paths (sparse view).
    text, c = count_replace(text, "  val rows = assetByPath.keys().mapBy(p =>", "  val rows = PATHS.filter(p => assetByPath.get(p) != 0).mapBy(p =>")
    counts["rows_proj"] = c

    # 3. Pre-chain sweep-removal filter (the action this slice rewrote has
    # since been deleted; kept so re-running the codemod on an old copy works).
    old_sweeproot = "    assetByPath' = assetByPath.keys().filter(p => not (rootOf(p) == r and not(files.get(p).present))).mapBy(k => assetByPath.get(k)),\n"
    new_sweeproot = "    assetByPath' = PATHS.mapBy(p => if (rootOf(p) == r and not(files.get(p).present)) { 0 } else { assetByPath.get(p) }),\n"
    text, c = count_replace(text, old_sweeproot, new_sweeproot)
    counts["sweeproot"] = c

    # 4. SweepRemoveBatchApply removal filter.
    old_srba = (
        "    assetByPath' = assetByPath.keys().filter(p =>\n"
        "      not (sweepBatch.get(p) != -1 and\n"
        "           assetByPath.keys().contains(p) and\n"
        "           not(files.get(p).present))\n"
        "    ).mapBy(k => assetByPath.get(k)),\n"
    )
    new_srba = (
        "    assetByPath' = PATHS.mapBy(p =>\n"
        "      if (sweepBatch.get(p) != -1 and\n"
        "          assetByPath.get(p) != 0 and\n"
        "          not(files.get(p).present)) { 0 } else { assetByPath.get(p) }\n"
        "    ),\n"
    )
    text, c = count_replace(text, old_srba, new_srba)
    counts["sweepremovebatchapply"] = c

    # 5. ApiRemove removal.
    text, c = count_replace(text, "    assetByPath' = assetByPath.keys().exclude(Set(p)).mapBy(k => assetByPath.get(k)),\n", "    assetByPath' = assetByPath.put(p, 0),\n")
    counts["apiremove"] = c

    # 6. ApiMove / MoveRowRewrite removal+move.
    text, c = count_replace(text, "    assetByPath' = assetByPath.keys().exclude(Set(p)).mapBy(k => assetByPath.get(k)).put(q, assetByPath.get(p)),\n", "    assetByPath' = assetByPath.put(p, 0).put(q, assetByPath.get(p)),\n")
    counts["apimove"] = c

    # 7. WorkerRemoveApply conditional removal.
    old_wra = "      assetByPath' = if (assetByPath.keys().contains(it.path)) { assetByPath.keys().exclude(Set(it.path)).mapBy(k => assetByPath.get(k)) } else { assetByPath },\n"
    new_wra = "      assetByPath' = if (assetByPath.get(it.path) != 0) { assetByPath.put(it.path, 0) } else { assetByPath },\n"
    text, c = count_replace(text, old_wra, new_wra)
    counts["workerremoveapply"] = c

    # 8. ExternalRenameDir / ApiAssignDirMove fold branches.
    old_fold = (
        "      if (acc.keys().contains(pr.src)) {\n"
        "        acc.keys().exclude(Set(pr.src)).mapBy(k => acc.get(k)).put(pr.dst, acc.get(pr.src))\n"
        "      } else {\n"
        "        acc.keys().exclude(Set(pr.src)).mapBy(k => acc.get(k))\n"
        "      }\n"
    )
    new_fold = (
        "      if (acc.get(pr.src) != 0) {\n"
        "        acc.put(pr.src, 0).put(pr.dst, acc.get(pr.src))\n"
        "      } else {\n"
        "        acc\n"
        "      }\n"
    )
    text, c = count_replace(text, old_fold, new_fold)
    counts["dirmove_fold"] = c

    # 9. Global presence rewrite (paren-balanced).
    text, c = rewrite_presence(text)
    counts["presence"] = c

    CORE.write_text(text)
    changed = text != original
    print("slice-assetbypath: counts = %s" % counts)
    print("slice-assetbypath: file changed = %s" % changed)
    return 0


if __name__ == "__main__":
    sys.exit(main())
