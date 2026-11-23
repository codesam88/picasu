#!/usr/bin/env python3
"""Codemod for Phase 1 slice 7d: nextAssetId var -> nextFreeId quotient.

The allocation counter is eliminated. Allocation reads a derived `nextFreeId`
(the smallest media id in 1..PATHS.size()+1 whose assetById record is absent),
and every assignment to the var, every frame stutter and the declaration are
deleted. `DbLoss`'s reset becomes vacuous with the counter gone.

Nothing but the counter references are touched: the allocator reads keep their
if/else shape with `nextFreeId` in the absent branch, and every other table's
frame conjuncts are left alone.
"""
import pathlib
import re
import sys

CORE = pathlib.Path(__file__).resolve().parents[1] / "inductive-core.qnt"

DECL = (
    "  // nextAssetId models asset-id allocation via generate_random_hash in model/asset.rs (AssetRecord::new_media/new_album) and flush_tree.rs; code uses random hashes, not a counter.\n"
    "  var nextAssetId: int\n"
)

INSERT_ANCHOR = "  // ---------- Environment injections ----------\n"

FREE_ID_DEF = (
    "  // Smallest free media id (SCOPE-17 quotient): allocation reads this\n"
    "  // instead of a counter. Reads the id table, like the code's allocator.\n"
    "  // With no free id left it returns PATHS.size() + 2, outside the id domain;\n"
    "  // the IdBudget conjunct is what rules that state out.\n"
    "  def nextFreeId: int =\n"
    "    1.to(PATHS.size() + 1).fold(PATHS.size() + 2, (acc, id) =>\n"
    "      if ((assetById.get(id).kind == \"absent\") and id < acc) id else acc\n"
    "    )\n"
)

# Whole-line deletions for the active assignments (they contain allocation
# reads, so they go before the read substitution).
ACTIVE_LINES = (
    "      nextAssetId' = if ((assetByPath.get(it.path) != 0)) { nextAssetId } else { nextAssetId + 1 },\n",
    "      nextAssetId' = if (rows.keys().contains(p)) { nextAssetId } else { nextAssetId + 1 },\n",
    "    nextAssetId' = PATHS.size() + 1,\n",
    "    nextAssetId' = 1,\n",
)


def main():
    text = CORE.read_text()
    original = text
    counts = {}

    # 1. Active assignments: init, DbLoss, WorkerApplySuccess, UploadIndexed.
    for line in ACTIVE_LINES:
        c = text.count(line)
        text = text.replace(line, "")
        counts[line.strip()] = c

    # 2. Standalone frame stutters.
    text, c = re.subn(r"(?m)^[ \t]*nextAssetId' = nextAssetId,\n", "", text)
    counts["stutter_lines"] = c

    # 3. Inline frame stutters inside single-line `.then(all { ... })` frames.
    c = text.count("nextAssetId' = nextAssetId, ")
    text = text.replace("nextAssetId' = nextAssetId, ", "")
    counts["stutter_inline"] = c

    # 4. Remaining allocation reads.
    c = text.count("else { nextAssetId }")
    text = text.replace("else { nextAssetId }", "else { nextFreeId }")
    counts["reads"] = c

    # 5. Declaration (idempotent: absent on a rerun).
    if DECL in text:
        text = text.replace(DECL, "", 1)
        counts["decl"] = 1
    else:
        counts["decl"] = 0

    # 6. Derived accessor (idempotent: present on a rerun).
    if "def nextFreeId" in text:
        counts["nextFreeId_def"] = 0
    else:
        assert INSERT_ANCHOR in text, "environment-injections anchor not found"
        text = text.replace(INSERT_ANCHOR, FREE_ID_DEF + "\n" + INSERT_ANCHOR, 1)
        counts["nextFreeId_def"] = 1

    residual = text.count("nextAssetId")
    CORE.write_text(text)
    changed = text != original
    print("slice-nextassetid: counts = %s" % counts)
    print("slice-nextassetid: file changed = %s" % changed)
    print("slice-nextassetid: residual `nextAssetId` = %d" % residual)
    return 0 if residual == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
