#!/usr/bin/env python3
"""Codemod for Phase 1 slice 4: assetById sparse -> total map IDS -> AssetRecord (sentinel kind == "absent")."""
import pathlib
import sys

CORE = pathlib.Path(__file__).resolve().parents[1] / "inductive-core.qnt"
IDS_EXPR = "0.to(PATHS.size() + 1)"


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
    needle = "assetById.keys().contains("
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
        out.append("(assetById.get(" + arg + ").kind != \"absent\")")
        count += 1
        i = close_idx + 1
    joined = "".join(out)
    # Canonical negated form: not((get(x).kind != "absent")) == get(x).kind == "absent"
    import re as _re
    joined, n_neg = _re.subn(
        r'not\(\((assetById\.get\([0-9A-Za-z_]+\)\.kind != "absent")\)\)',
        lambda m: m.group(1).replace("!=", "=="),
        joined,
    )
    return joined, count + n_neg


def rewrite_exclude_removal(text):
    """`assetById.keys().exclude(X).mapBy(k => assetById.get(k))` -> sentinel wipe."""
    needle = "assetById.keys().exclude("
    tail = ".mapBy(k => assetById.get(k))"
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
        if close_idx == -1 or not text.startswith(tail, close_idx + 1):
            out.append(text[idx:close_idx + 1 if close_idx != -1 else idx + len(needle)])
            i = (close_idx + 1) if close_idx != -1 else idx + len(needle)
            continue
        arg = text[open_idx + 1:close_idx]
        out.append(
            IDS_EXPR
            + ".mapBy(id => if (id.in("
            + arg
            + ")) { absentAssetRecord } else { assetById.get(id) })"
        )
        count += 1
        i = close_idx + 1 + len(tail)
    return "".join(out), count


ABSENT_DEF = (
    "\n"
    "  // Absent-row sentinel for the total-map redesign (assetById: IDS -> AssetRecord).\n"
    "  // Shape mirrors initialAlbumAssetRecord with kind \"absent\"; reused for every\n"
    "  // unwritten id slot and for removal wipes.\n"
    "  pure def absentAssetRecord: AssetRecord = {\n"
    "    assetId: 0,\n"
    "    kind: \"absent\",\n"
    "    path: \"\",\n"
    "    contentHash: -1,\n"
    "    albumId: -1,\n"
    "    isTrashed: false\n"
    "  }\n"
)

ALBUM_BLOCK = (
    "  pure def initialAlbumAssetRecord: AssetRecord = {\n"
    "    assetId: 0,\n"
    "    kind: \"album\",\n"
    "    path: \"\",\n"
    "    contentHash: -1,\n"
    "    albumId: -1,\n"
    "    isTrashed: false\n"
    "  }\n"
)


def main():
    text = CORE.read_text()
    original = text
    counts = {}

    # 0. Add absentAssetRecord def once (idempotent).
    if "pure def absentAssetRecord: AssetRecord" not in text:
        assert ALBUM_BLOCK in text, "album block not found"
        text = text.replace(ALBUM_BLOCK, ALBUM_BLOCK + ABSENT_DEF, 1)
        counts["absent_def"] = 1
    else:
        counts["absent_def"] = 0

    # 1. init initializer: total map with album at 0, path rows, absent elsewhere.
    old_init = "    assetById' = PATHS.fold(Map(0 -> initialAlbumAssetRecord), (acc, p) => acc.put(initialId(p), initialAssetRecord(p))),\n"
    new_init = "    assetById' = PATHS.fold(" + IDS_EXPR + ".mapBy(id => if (id == 0) initialAlbumAssetRecord else absentAssetRecord), (acc, p) => acc.put(initialId(p), initialAssetRecord(p))),\n"
    text, c = count_replace(text, old_init, new_init)
    counts["init"] = c

    # 2. DbLoss wipe: all-absent total map.
    text, c = count_replace(text, "    assetById' = Map(),\n", "    assetById' = " + IDS_EXPR + ".mapBy(id => absentAssetRecord),\n")
    counts["dbloss"] = c

    # 3. ApiRemove removal.
    text, c = count_replace(text, "    assetById' = assetById.keys().exclude(Set(assetByPath.get(p))).mapBy(k => assetById.get(k)),\n",
                             "    assetById' = assetById.put(assetByPath.get(p), absentAssetRecord),\n")
    counts["apiremove"] = c

    # 4. WorkerRemoveApply conditional removal.
    old_wra = "      assetById' = if ((assetByPath.get(it.path) != 0)) { assetById.keys().exclude(Set(assetByPath.get(it.path))).mapBy(k => assetById.get(k)) } else { assetById },\n"
    new_wra = "      assetById' = if ((assetByPath.get(it.path) != 0)) { assetById.put(assetByPath.get(it.path), absentAssetRecord) } else { assetById },\n"
    text, c = count_replace(text, old_wra, new_wra)
    counts["workerremoveapply"] = c

    # 5. RemoveIdOnly removal.
    # same string as ApiRemove; already replaced by step 3 (count may be 0 here).

    # 6. SweepRemoveBatchApply filter-removal (total-preserving).
    old_srba = (
        "    assetById' = assetById.keys().filter(id =>\n"
        "      not (PATHS.exists(p => sweepBatch.get(p) != -1 and\n"
        "        (assetByPath.get(p) != 0) and\n"
        "        assetByPath.get(p) == id and\n"
        "        not(files.get(p).present)))\n"
        "    ).mapBy(k => assetById.get(k)),\n"
    )
    new_srba = (
        "    assetById' = " + IDS_EXPR + ".mapBy(id =>\n"
        "      if (PATHS.exists(p => sweepBatch.get(p) != -1 and\n"
        "        (assetByPath.get(p) != 0) and\n"
        "        assetByPath.get(p) == id and\n"
        "        not(files.get(p).present))) { absentAssetRecord } else { assetById.get(id) }\n"
        "    ),\n"
    )
    text, c = count_replace(text, old_srba, new_srba)
    counts["sweepremovebatchapply"] = c

    # 6b. Move-retraction removal (REQ-17): exclude a set of displaced ids.
    text, counts["retraction"] = rewrite_exclude_removal(text)

    # 7. AssetIdKeyCoherent invariant: sentinel form.
    old_akc = "  val AssetIdKeyCoherent: bool =\n    assetById.keys().forall(id => assetById.get(id).assetId == id)\n"
    new_akc = "  val AssetIdKeyCoherent: bool =\n    " + IDS_EXPR + ".forall(id => assetById.get(id).kind == \"absent\" or assetById.get(id).assetId == id)\n"
    text, c = count_replace(text, old_akc, new_akc)
    counts["assetidkeycoherent"] = c

    # 8. forall iteration sites (remaining).
    text, c = count_replace(text, "assetById.keys().forall(", IDS_EXPR + ".forall(")
    counts["forall"] = c

    # 9. filter iteration sites (AlbumStats nested).
    text, c = count_replace(text, "assetById.keys().filter(m =>", IDS_EXPR + ".filter(m =>")
    counts["filter_m"] = c

    # 10. presence tests (balanced).
    text, c = rewrite_presence(text)
    counts["presence"] = c

    CORE.write_text(text)
    changed = text != original
    print("slice-assetbyid: counts = %s" % counts)
    print("slice-assetbyid: file changed = %s" % changed)
    return 0


if __name__ == "__main__":
    sys.exit(main())
