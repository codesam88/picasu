#!/usr/bin/env python3
"""Codemod for Phase 1 slice 5: metadataPayload sparse -> total map IDS -> MetadataRecord (sentinel kind == "absent")."""
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
    needle = "metadataPayload.keys().contains("
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
        out.append("(metadataPayload.get(" + arg + ").kind != \"absent\")")
        count += 1
        i = close_idx + 1
    return "".join(out), count


def rewrite_exclude_removal(text):
    """`metadataPayload.keys().exclude(X).mapBy(k => metadataPayload.get(k))` -> sentinel wipe."""
    needle = "metadataPayload.keys().exclude("
    tail = ".mapBy(k => metadataPayload.get(k))"
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
            + ")) { absentMetadataRecord } else { metadataPayload.get(id) })"
        )
        count += 1
        i = close_idx + 1 + len(tail)
    return "".join(out), count


ABSENT_DEF = (
    "\n"
    "  // Absent-row sentinel for the total-map redesign (metadataPayload: IDS -> MetadataRecord).\n"
    "  // Shape mirrors initialAlbumMetadataRecord with kind \"absent\"; reused for every\n"
    "  // unwritten id slot and for removal wipes.\n"
    "  pure def absentMetadataRecord: MetadataRecord = {\n"
    "    kind: \"absent\",\n"
    "    ver: 0,\n"
    "    fileSize: 0,\n"
    "    converged: false,\n"
    "    itemCount: 0,\n"
    "    itemSize: 0,\n"
    "    hasShareList: false\n"
    "  }\n"
)

ALBUM_BLOCK = (
    "  pure def initialAlbumMetadataRecord: MetadataRecord = {\n"
    "    kind: \"album\",\n"
    "    ver: 0,\n"
    "    fileSize: 0,\n"
    "    converged: true,\n"
    "    itemCount: PATHS.size(),\n"
    "    itemSize: 0,\n"
    "    hasShareList: false\n"
    "  }\n"
)


def main():
    text = CORE.read_text()
    original = text
    counts = {}

    # 0. Add absentMetadataRecord def once (idempotent).
    if "pure def absentMetadataRecord: MetadataRecord" not in text:
        assert ALBUM_BLOCK in text, "album block not found"
        text = text.replace(ALBUM_BLOCK, ALBUM_BLOCK + ABSENT_DEF, 1)
        counts["absent_def"] = 1
    else:
        counts["absent_def"] = 0

    # 1. init initializer: total map with album at 0, path rows, absent elsewhere.
    old_init = "    metadataPayload' = PATHS.fold(Map(0 -> initialAlbumMetadataRecord), (acc, p) => acc.put(initialId(p), initialMetadataRecord(p))),\n"
    new_init = "    metadataPayload' = PATHS.fold(" + IDS_EXPR + ".mapBy(id => if (id == 0) initialAlbumMetadataRecord else absentMetadataRecord), (acc, p) => acc.put(initialId(p), initialMetadataRecord(p))),\n"
    text, c = count_replace(text, old_init, new_init)
    counts["init"] = c

    # 2. DbLoss wipe: all-absent total map.
    text, c = count_replace(text, "    metadataPayload' = Map(),\n", "    metadataPayload' = " + IDS_EXPR + ".mapBy(id => absentMetadataRecord),\n")
    counts["dbloss"] = c

    # 3. WorkerRemoveApply removal (album-put suffix preserved).
    text, c = count_replace(
        text,
        "metadataPayload.keys().exclude(Set(assetByPath.get(it.path))).mapBy(k => metadataPayload.get(k))",
        "metadataPayload.put(assetByPath.get(it.path), absentMetadataRecord)",
    )
    counts["workerremoveapply"] = c

    # 4. ApiRemove and RemoveIdOnly removals (album-put suffix preserved on the latter).
    text, c = count_replace(
        text,
        "metadataPayload.keys().exclude(Set(assetByPath.get(p))).mapBy(k => metadataPayload.get(k))",
        "metadataPayload.put(assetByPath.get(p), absentMetadataRecord)",
    )
    counts["apiremove_removeidonly"] = c

    # 4b. Move-retraction removal (REQ-17): exclude a set of displaced ids.
    text, counts["retraction"] = rewrite_exclude_removal(text)

    # 5. SweepRemoveBatchApply filter-removal (total-preserving).
    old_srba = (
        "    metadataPayload' = metadataPayload.keys().filter(id =>\n"
        "      not (PATHS.exists(p => sweepBatch.get(p) != -1 and\n"
        "        (assetByPath.get(p) != 0) and\n"
        "        assetByPath.get(p) == id and\n"
        "        not(files.get(p).present)))\n"
        "    ).mapBy(k => metadataPayload.get(k))\n"
    )
    new_srba = (
        "    metadataPayload' = " + IDS_EXPR + ".mapBy(id =>\n"
        "      if (PATHS.exists(p => sweepBatch.get(p) != -1 and\n"
        "        (assetByPath.get(p) != 0) and\n"
        "        assetByPath.get(p) == id and\n"
        "        not(files.get(p).present))) { absentMetadataRecord } else { metadataPayload.get(id) }\n"
        "    )\n"
    )
    text, c = count_replace(text, old_srba, new_srba)
    counts["sweepremovebatchapply"] = c

    # 6. presence tests (balanced); album-row guards keep their form verbatim
    #    (get(0).kind != "absent"), per the A3 encoding note.
    text, c = rewrite_presence(text)
    counts["presence"] = c

    CORE.write_text(text)
    changed = text != original
    residual = text.count("metadataPayload.keys()")
    print("slice-metadatapayload: counts = %s" % counts)
    print("slice-metadatapayload: file changed = %s" % changed)
    print("slice-metadatapayload: residual metadataPayload.keys() = %d" % residual)
    return 0


if __name__ == "__main__":
    sys.exit(main())
