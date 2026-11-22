#!/usr/bin/env python3
"""Codemod for Phase 1 slice 2: sweepBatch sparse -> total map PATHS -> int (sentinel -1)."""
import pathlib
import sys

CORE = pathlib.Path(__file__).resolve().parents[1] / "inductive-core.qnt"


def count_replace(text, old, new):
    c = text.count(old)
    if c:
        text = text.replace(old, new)
    return text, c


def main():
    text = CORE.read_text()
    original = text
    counts = {}

    # 1. Map() initializers -> total all-(-1).
    text, c = count_replace(text, "sweepBatch' = Map()", "sweepBatch' = PATHS.mapBy(p => -1)")
    counts["map_init"] = c

    # 2. SweepEnumerate accumulation block.
    old_enum = (
        "    sweepBatch' = PATHS.filter(p => rootOf(p) == root and assetByPath.keys().contains(p) and not(files.get(p).present))\n"
        "      .fold(\n"
        "        sweepBatch.keys().filter(p => not(rootOf(p) == root)).mapBy(p => sweepBatch.get(p)),\n"
        "        (acc, p) => acc.put(p, generations.get(p))\n"
        "      ),\n"
    )
    new_enum = (
        "    sweepBatch' = PATHS.mapBy(p =>\n"
        "      if (rootOf(p) == root and assetByPath.keys().contains(p) and not(files.get(p).present)) {\n"
        "        generations.get(p)\n"
        "      } else {\n"
        "        if (not(rootOf(p) == root)) {\n"
        "          sweepBatch.get(p)\n"
        "        } else {\n"
        "          -1\n"
        "        }\n"
        "      }\n"
        "    ),\n"
    )
    text, c = count_replace(text, old_enum, new_enum)
    counts["sweepenumerate"] = c

    # 3. SweepRecheck block.
    old_recheck = (
        "    sweepBatch.keys().size() > 0,\n"
        "    sweepBatch' = sweepBatch.keys().filter(p =>\n"
        "      sweepBatch.get(p) == generations.get(p) and\n"
        "      not(p.in(leasedPaths)) and\n"
        "      not(files.get(p).present)\n"
        "    ).mapBy(p => sweepBatch.get(p)),\n"
    )
    new_recheck = (
        "    PATHS.exists(p => sweepBatch.get(p) != -1),\n"
        "    sweepBatch' = PATHS.mapBy(p =>\n"
        "      if (sweepBatch.get(p) != -1 and\n"
        "          sweepBatch.get(p) == generations.get(p) and\n"
        "          not(p.in(leasedPaths)) and\n"
        "          not(files.get(p).present)) {\n"
        "        sweepBatch.get(p)\n"
        "      } else {\n"
        "        -1\n"
        "      }\n"
        "    ),\n"
    )
    text, c = count_replace(text, old_recheck, new_recheck)
    counts["sweeprecheck"] = c

    # 4. SweepRemoveBatchApply specific edits.
    text, c = count_replace(text, "    sweepBatch.keys().size() > 0,\n", "    PATHS.exists(p => sweepBatch.get(p) != -1),\n")
    counts["apply_size"] = c

    text, c = count_replace(text, "      not (sweepBatch.keys().contains(p) and\n", "      not (sweepBatch.get(p) != -1 and\n")
    counts["apply_contains"] = c

    old_exists = (
        "      not (sweepBatch.keys().exists(p =>\n"
        "        assetByPath.keys().contains(p) and\n"
        "        assetByPath.get(p) == id and\n"
        "        not(files.get(p).present)))\n"
    )
    new_exists = (
        "      not (PATHS.exists(p => sweepBatch.get(p) != -1 and\n"
        "        assetByPath.keys().contains(p) and\n"
        "        assetByPath.get(p) == id and\n"
        "        not(files.get(p).present)))\n"
    )
    text, c = count_replace(text, old_exists, new_exists)
    counts["apply_exists"] = c

    old_filter = "sweepBatch.keys().filter(p => assetByPath.keys().contains(p) and not(files.get(p).present) and assetById.keys().contains(assetByPath.get(p)) and assetById.get(assetByPath.get(p)).albumId == 0 and not(assetById.get(assetByPath.get(p)).isTrashed))"
    new_filter = "PATHS.filter(p => sweepBatch.get(p) != -1 and assetByPath.keys().contains(p) and not(files.get(p).present) and assetById.keys().contains(assetByPath.get(p)) and assetById.get(assetByPath.get(p)).albumId == 0 and not(assetById.get(assetByPath.get(p)).isTrashed))"
    text, c = count_replace(text, old_filter, new_filter)
    counts["apply_filter"] = c

    # 5. Val invariants.
    old_inv1 = "val SweepRemoveBatchSkipsPresentFile = sweepBatch.keys().forall(p => not(files.get(p).present) or not(sweepBatch.keys().contains(p)))"
    new_inv1 = "val SweepRemoveBatchSkipsPresentFile = PATHS.forall(p => sweepBatch.get(p) == -1 or not(files.get(p).present))"
    text, c = count_replace(text, old_inv1, new_inv1)
    counts["inv1"] = c

    old_inv2 = "val SweepRecheckBatchClean = sweepBatch.keys().forall(p => sweepBatch.get(p) == generations.get(p) and not(p.in(leasedPaths)) and not(files.get(p).present))"
    new_inv2 = "val SweepRecheckBatchClean = PATHS.forall(p => sweepBatch.get(p) == -1 or (sweepBatch.get(p) == generations.get(p) and not(p.in(leasedPaths)) and not(files.get(p).present)))"
    text, c = count_replace(text, old_inv2, new_inv2)
    counts["inv2"] = c

    # 6. Quiescent emptiness.
    text, c = count_replace(text, "    sweepBatch.keys().size() == 0 and\n", "    PATHS.forall(p => sweepBatch.get(p) == -1) and\n")
    counts["quiescent"] = c

    # 7. Test assertions.
    text, c = count_replace(text, "sweepBatch.keys() == Set()", "PATHS.filter(p => sweepBatch.get(p) != -1) == Set()")
    counts["test_empty"] = c
    text, c = count_replace(text, 'sweepBatch.keys() == Set("p2")', 'PATHS.filter(p => sweepBatch.get(p) != -1) == Set("p2")')
    counts["test_p2"] = c

    CORE.write_text(text)
    changed = text != original
    print("slice-sweepbatch: counts = %s" % counts)
    print("slice-sweepbatch: file changed = %s" % changed)
    return 0


if __name__ == "__main__":
    sys.exit(main())
