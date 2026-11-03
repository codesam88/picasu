#!/usr/bin/env python3
"""Aggregate rust-code-analysis metrics per file and per function.

Usage:
    scripts/code-complexity.py foo/bar/baz.rs [other.rs ...] [dir/ ...]

Runs rust-code-analysis-cli (-m) over the given Rust files (directories
are expanded to **/*.rs) and prints one table row per file plus one row
per function inside it:

    Component            SLOC  Cyclo  Halstead V    MI  Cognitive
    foo/bar/baz.rs        234     23         437  52.1         12
       handle_x            23     34         ...   ...         34

Columns: SLOC = source lines; Cyclo = McCabe sum; Halstead V =
Halstead volume; MI = Maintainability Index, Visual-Studio variant
(0-100, higher is better); Cognitive = the tool's cognitive sum.
NOTE: the CI gate for cognitive complexity remains
`cargo clippy -W clippy::cognitive_complexity` (same toolchain as
`just backend-check`); this table uses the single rust-code-analysis
parser so every computed mix comes from one source.

Raw JSON is kept only in a temporary directory (see --keep-json).
Exit non-zero if the analyzer binary is missing or a file fails.
"""

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

BINARY = "rust-code-analysis-cli"

# A line starting a test module: optional #[cfg(test)] attribute lines
# immediately followed by `mod tests {`.
TEST_MOD_RE = re.compile(r"^\s*mod\s+tests\s*\{")
CFG_TEST_RE = re.compile(r"^\s*#\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*$")
TEST_ATTR_RE = re.compile(r"^\s*#\[\s*test\s*\]\s*$")
FN_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+\w+")


def strip_tests(source: str) -> str:
    """Remove `#[cfg(test)] mod tests { ... }` blocks and free `#[test]` fns.

    Line-based brace matching that skips // comments, /* */ comments,
    string/byte-string literals and char literals (with escapes), so
    braces inside them do not affect the depth count. Returns the
    source with test blocks excised (line count preserved: removed
    lines become blank, keeping analyzer spans comparable).
    """
    lines = source.split("\n")
    remove = [False] * len(lines)
    i = 0
    n = len(lines)
    while i < n:
        is_test_mod = bool(TEST_MOD_RE.match(lines[i])) and any(
            CFG_TEST_RE.match(lines[j])
            for j in range(max(0, i - 3), i)
            if lines[j].strip() and not lines[j].strip().startswith("//")
        )
        is_test_fn = False
        fn_idx = None
        if TEST_ATTR_RE.match(lines[i]):
            for j in range(i + 1, min(n, i + 6)):
                if not lines[j].strip() or lines[j].strip().startswith("//"):
                    continue
                if lines[j].strip().startswith("#["):
                    continue
                if FN_RE.match(lines[j]):
                    is_test_fn = True
                    fn_idx = j
                break
        if not (is_test_mod or is_test_fn):
            i += 1
            continue
        start = i
        # Excise from the attribute/test-mod line through the matching
        # close brace of the block that opens at or after fn_idx/i.
        depth = 0
        opened = False
        in_block_comment = False
        j = fn_idx if fn_idx is not None else i
        while j < n:
            line = lines[j]
            k = 0
            in_string: str | None = None
            raw_hashes = 0
            in_char = False
            in_line_comment = False
            while k < len(line):
                c = line[k]
                nxt = line[k + 1] if k + 1 < len(line) else ""
                if in_line_comment:
                    break
                if in_block_comment:
                    if c == "*" and nxt == "/":
                        in_block_comment = False
                        k += 2
                        continue
                    k += 1
                    continue
                if in_string:
                    if raw_hashes:
                        closing = "#" * raw_hashes
                        if c == '"' and line[k + 1 : k + 1 + raw_hashes] == closing:
                            in_string = None
                            raw_hashes = 0
                            k += 1 + len(closing)
                            continue
                        k += 1
                        continue
                    if c == "\\":
                        k += 2
                        continue
                    if c == in_string:
                        in_string = None
                    k += 1
                    continue
                if in_char:
                    if c == "\\":
                        k += 2
                        continue
                    if c == "'":
                        in_char = False
                    k += 1
                    continue
                if c == "/" and nxt == "/":
                    in_line_comment = True
                    break
                if c == "/" and nxt == "*":
                    in_block_comment = True
                    k += 2
                    continue
                if c == "b" and nxt in ('"', "r"):
                    # b"..." or br"..." raw byte string.
                    rest = line[k + 1 :]
                    if rest.startswith("r"):
                        m = re.match(r'r(#+)"', rest)
                        if m:
                            in_string = '"'
                            raw_hashes = len(m.group(1))
                            k += 1 + len(m.group(0))
                            continue
                    in_string = '"'
                    k += 2
                    continue
                if c == "r":
                    m = re.match(r'r(#+)?"', line[k:])
                    if m:
                        in_string = '"'
                        raw_hashes = len(m.group(1) or "")
                        k += len(m.group(0))
                        continue
                    k += 1
                    continue
                if c == '"':
                    in_string = c
                    k += 1
                    continue
                if c == "'":
                    # Char literal only: 'x' or escaped '\..'. A lone '
                    # is a lifetime annotation — ignore it.
                    rest = line[k:]
                    if re.match(r"'(\\.|[^'\\])'", rest):
                        k += 4 if rest[1] == "\\" else 3
                        continue
                    k += 1
                    continue
                if c == "{":
                    depth += 1
                    opened = True
                elif c == "}":
                    depth -= 1
                k += 1
            if opened and depth <= 0:
                for t in range(start, j + 1):
                    remove[t] = True
                i = j + 1
                break
            j += 1
        else:
            # No block found; drop just the attribute line.
            remove[start] = True
            i = start + 1
    return "\n".join("" if remove[idx] else line for idx, line in enumerate(lines))


def collect_rust_files(paths: list[str]) -> list[Path]:
    files: list[Path] = []
    for raw in paths:
        p = Path(raw)
        if p.is_dir():
            files.extend(sorted(p.rglob("*.rs")))
        elif p.is_file():
            files.append(p)
        else:
            print(f"error: no such file or directory: {raw}", file=sys.stderr)
            sys.exit(2)
    # De-duplicate, keep order.
    seen: set[Path] = set()
    unique: list[Path] = []
    for f in files:
        if f not in seen:
            seen.add(f)
            unique.append(f)
    return unique


def analyze(pairs: list[tuple[Path, Path]], outdir: Path) -> dict[str, dict]:
    """Run the analyzer on actual paths; key results by display path.

    `pairs` is (display_path, actual_file_to_analyze). The analyzer
    names blobs by file stem, so callers resolve blobs back to display
    paths (see main()).
    """
    cmd = [BINARY, "-m", "-O", "json", "-o", str(outdir)]
    for _, actual in pairs:
        cmd.extend(["-p", str(actual)])
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        print(f"error: {BINARY} failed:\n{proc.stderr}", file=sys.stderr)
        sys.exit(1)
    results: dict[str, dict] = {}
    for blob in outdir.rglob("*.json"):
        try:
            results.setdefault(blob.stem, json.loads(blob.read_text()))
        except (json.JSONDecodeError, OSError) as exc:
            print(f"error: cannot parse {blob}: {exc}", file=sys.stderr)
            sys.exit(1)
    return results


def function_rows(node: dict, prefix: str = "   ") -> list[tuple]:
    """Flatten spaces of kind function/impl into (name, metrics) rows.

    The analyzer emits each method twice (once standalone, once under
    its impl); exact duplicates are dropped, keeping the qualified
    (impl::method) form.
    """
    rows: list[tuple] = []
    for space in node.get("spaces", []):
        kind = space.get("kind", "")
        name = space.get("name", "?")
        if kind == "function":
            rows.append((f"{prefix}{name}", space.get("metrics", {})))
        elif kind in ("impl", "trait"):
            for sub in space.get("spaces", []):
                if sub.get("kind") == "function":
                    rows.append(
                        (f"{prefix}{name}::{sub.get('name', '?')}", sub.get("metrics", {}))
                    )
            # Recurse one more level for nested modules inside impls (rare).
            rows.extend(function_rows(space, prefix))
    # The analyzer emits each method twice (standalone + under impl),
    # sometimes with context-dependent metric wrappers that differ in
    # full JSON but agree on the displayed columns. Dedupe on those
    # columns, keeping the qualified (impl::method) form.
    def cols(metrics: dict) -> tuple:
        loc = metrics.get("loc", {})
        return (
            round(float(loc.get("sloc", 0))),
            round(float(metrics.get("cyclomatic", {}).get("sum", 0))),
            round(float(metrics.get("halstead", {}).get("volume", 0))),
            round(float(metrics.get("mi", {}).get("mi_visual_studio", 0)), 1),
            round(float(metrics.get("cognitive", {}).get("sum", 0))),
        )

    rows.sort(key=lambda r: ("::" not in r[0], r[0]))
    deduped: list[tuple] = []
    seen: set = set()
    for row_name, metrics in rows:
        key = (row_name.split("::")[-1].strip(), cols(metrics))
        if key not in seen:
            seen.add(key)
            deduped.append((row_name, metrics))
    return deduped


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", help=".rs files or directories")
    parser.add_argument(
        "--keep-json",
        default=None,
        help="keep raw analyzer JSON under this dir (default: temp dir, removed)",
    )
    parser.add_argument(
        "--include-tests",
        action="store_true",
        help="analyze test code too (default: #[cfg(test)] mod tests and "
        "#[test] fns are stripped before analysis; stripped copies keep "
        "line numbers so spans stay comparable)",
    )
    parser.add_argument(
        "--functions",
        action="store_true",
        help="also print one row per function (default: per-file rows only)",
    )
    args = parser.parse_args()

    if shutil.which(BINARY) is None:
        print(
            f"error: {BINARY} not found on PATH "
            "(cargo install rust-code-analysis-cli)",
            file=sys.stderr,
        )
        sys.exit(1)

    files = collect_rust_files(args.paths)
    if not files:
        print("error: no .rs files found", file=sys.stderr)
        sys.exit(2)

    def run(pairs: list[tuple[Path, Path]]) -> dict[str, dict]:
        if args.keep_json:
            outdir = Path(args.keep_json)
            outdir.mkdir(parents=True, exist_ok=True)
            return analyze(pairs, outdir)
        with tempfile.TemporaryDirectory(prefix="code-complexity-") as tmp:
            return analyze(pairs, Path(tmp))

    if args.include_tests:
        pairs = [(f, f) for f in files]
        results = run(pairs)
        display = {f: f for f in files}
    else:
        with tempfile.TemporaryDirectory(prefix="code-complexity-src-") as tmp:
            pairs = []
            display = {}
            for f in files:
                stripped = strip_tests(f.read_text())
                actual = Path(tmp) / f.name
                # Avoid stem collision across dirs inside one analyzer run.
                suffix = 0
                while actual.exists():
                    suffix += 1
                    actual = Path(tmp) / f"{f.stem}-{suffix}.rs"
                actual.write_text(stripped)
                pairs.append((f, actual))
                display[f] = f
            results = run(pairs)

    # Analyzer blobs are keyed by analyzed-file stem; remap to display paths
    # (stripped copies may carry a numeric suffix against collisions).
    by_display: dict[Path, dict] = {}
    for display, actual in pairs:
        doc = results.get(actual.stem)
        if doc is None:
            for blob_doc in results.values():
                if blob_doc.get("name", "").endswith(str(actual)):
                    doc = blob_doc
                    break
        if doc is not None:
            by_display[display] = doc
    def row_values(metrics: dict) -> dict:
        loc = metrics.get("loc", {})
        return {
            "sloc": loc.get("sloc", 0),
            "cyclomatic": metrics.get("cyclomatic", {}).get("sum", 0),
            "halstead_volume": metrics.get("halstead", {}).get("volume", 0),
            "mi": metrics.get("mi", {}).get("mi_visual_studio", 0),
            "cognitive": metrics.get("cognitive", {}).get("sum", 0),
        }

    # Capture everything in memory first: a list of file records, each
    # with its metrics and (optionally) per-function metrics. All
    # output formats render from this structure.
    report: list[dict] = []
    for f in files:
        doc = by_display.get(f)
        if doc is None:
            # Fall back: search blobs whose recorded name matches.
            for blob_doc in results.values():
                if blob_doc.get("name", "").endswith(str(f)):
                    doc = blob_doc
                    break
        if doc is None:
            print(f"warning: no analyzer output for {f}", file=sys.stderr)
            continue
        record: dict = {"file": str(f), "metrics": row_values(doc.get("metrics", {}))}
        if args.functions:
            record["functions"] = [
                {"name": name, "metrics": row_values(fm)}
                for name, fm in function_rows(doc)
            ]
        report.append(record)

    render_table(report)


def render_table(report: list[dict]) -> None:
    """Print the human-readable aligned table for an in-memory report."""
    rows: list[tuple[str, dict]] = []
    for record in report:
        rows.append((record["file"], record["metrics"]))
        for fn in record.get("functions", []):
            rows.append((fn["name"], fn["metrics"]))
    if not rows:
        return
    width = max([len("Component")] + [len(name) for name, _ in rows])

    def fmt_row(name: str, vals: dict) -> str:
        return (
            f"{name:<{width}}"
            f"{vals['sloc']:>8.0f}"
            f"{vals['cyclomatic']:>8.0f}"
            f"{vals['halstead_volume']:>12.0f}"
            f"{vals['mi']:>8.1f}"
            f"{vals['cognitive']:>11.0f}"
        )

    print(
        f"{'Component':<{width}}{'SLOC':>8}{'Cyclo':>8}"
        f"{'Halstead V':>12}{'MI':>8}{'Cognitive':>11}"
    )
    for name, vals in rows:
        print(fmt_row(name, vals))


if __name__ == "__main__":
    main()
