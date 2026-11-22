#!/usr/bin/env python3
"""Codemod for Phase 1 slice 1: thumbPresent sparse -> total map HASHES -> bool."""
import pathlib
import sys

CORE = pathlib.Path(__file__).resolve().parents[1] / "inductive-core.qnt"

BARE_ANCHOR = "  var thumbPresent: int -> bool\n"
ACCESSOR_BLOCK = (
    "  var thumbPresent: int -> bool\n"
    "  // Accessor pair for the total-map redesign (thumbPresent: HASHES -> bool).\n"
    "  // Presence is value truthiness: absent reads as false.\n"
    "  def thumbHas(h: int): bool = thumbPresent.get(h)\n"
    "  def thumbGet(h: int): bool = thumbPresent.get(h)\n"
)


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


def collapse_doubled(text):
    count = 0

    # Case 1: thumbPresent.get(A) <ws> and <ws> thumbPresent.get(A) -> thumbPresent.get(A)
    needle = "thumbPresent.get("
    start = 0
    while True:
        idx = text.find(needle, start)
        if idx == -1:
            break
        open1 = idx + len("thumbPresent.get")
        close1 = read_balanced(text, open1)
        if close1 == -1:
            start = idx + 1
            continue
        arg1 = text[open1 + 1:close1]
        j = close1 + 1
        while j < len(text) and text[j].isspace():
            j += 1
        if text[j:j + 3] == "and":
            j += 3
            while j < len(text) and text[j].isspace():
                j += 1
            if text.startswith("thumbPresent.get(", j):
                open2 = j + len("thumbPresent.get")
                close2 = read_balanced(text, open2)
                if close2 != -1:
                    arg2 = text[open2 + 1:close2]
                    if arg1 == arg2:
                        collapsed = "thumbPresent.get(" + arg1 + ")"
                        text = text[:idx] + collapsed + text[close2 + 1:]
                        count += 1
                        start = idx + len(collapsed)
                        continue
        start = idx + 1

    # Case 2: not(thumbPresent.get(A)) <ws> or <ws> not(thumbPresent.get(A)) -> not(thumbPresent.get(A))
    needle2 = "not(thumbPresent.get("
    start = 0
    while True:
        idx = text.find(needle2, start)
        if idx == -1:
            break
        open1 = idx + len(needle2) - 1
        close_get1 = read_balanced(text, open1)
        if close_get1 == -1 or close_get1 + 1 >= len(text) or text[close_get1 + 1] != ")":
            start = idx + 1
            continue
        arg1 = text[open1 + 1:close_get1]
        pos = close_get1 + 2
        while pos < len(text) and text[pos].isspace():
            pos += 1
        if text[pos:pos + 2] == "or":
            pos += 2
            while pos < len(text) and text[pos].isspace():
                pos += 1
            if text.startswith("not(thumbPresent.get(", pos):
                open2 = pos + len("not(thumbPresent.get")  # points at '('? 
                # recompute open2 as last char of prefix
                open2 = pos + len("not(thumbPresent.get(") - 1
                close_get2 = read_balanced(text, open2)
                if close_get2 != -1 and close_get2 + 1 < len(text) and text[close_get2 + 1] == ")":
                    arg2 = text[open2 + 1:close_get2]
                    if arg1 == arg2:
                        collapsed = "not(thumbPresent.get(" + arg1 + "))"
                        end = close_get2 + 2
                        text = text[:idx] + collapsed + text[end:]
                        count += 1
                        start = idx + len(collapsed)
                        continue
        start = idx + 1

    return text, count


def main():
    text = CORE.read_text()
    original = text

    if ACCESSOR_BLOCK in text:
        text = text.replace(ACCESSOR_BLOCK, BARE_ANCHOR, 1)

    before_presence = text.count("thumbPresent.keys().contains(")
    text = text.replace("thumbPresent.keys().contains(", "thumbPresent.get(")

    text = text.replace("thumbPresent' = Map()", "thumbPresent' = HASHES.mapBy(h => false)")
    text = text.replace("thumbPresent = Map()", "thumbPresent = HASHES.mapBy(h => false)")

    text, collapsed_count = collapse_doubled(text)

    CORE.write_text(text)

    changed = text != original
    print("slice-thumbpresent: presence sites rewritten = %d" % before_presence)
    print("slice-thumbpresent: doubled conjuncts collapsed = %d" % collapsed_count)
    print("slice-thumbpresent: file changed = %s" % changed)
    return 0


if __name__ == "__main__":
    sys.exit(main())
