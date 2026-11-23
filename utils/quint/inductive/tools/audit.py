#!/usr/bin/env python3
"""Standing invariant audit for the inductive core's per-table rewrites.

The core is a derived artifact: each table is rewritten from a sparse map to a
total map over a constant finite key set, one codemod slice per table.  This
script checks the properties every landed slice must keep, so a later slice
cannot silently undo an earlier one.

Checks, per landed table T in {thumbPresent, sweepBatch, assetByPath,
assetById, metadataPayload, dupeIndex, files, generations}:

a. No `T.keys()` site remains, except a key-domain assertion (`T.keys() ==
   DOMAIN` / `!=`).  Totality made every presence test a sentinel read, so a
   `keys()` call is either a missed rewrite or a reintroduced sparse view;
   `files` and `generations` carry their domain assertion in `WithinBounds`,
   which is a fact about the model, not a sparse iteration.
b. No dead definitions: every `def` / `pure def` declared in the core is
   referenced elsewhere in the file.  `ThumbPresent` (core line 1580) is
   declared but unreferenced in the core and in the source model; it is the
   aspect observer named by the traceability registry and predates the
   redesign, so it is exempt.  Any other unused def fails the check.
c. Sentinel coherence:
   - no sparse initializer (`T' = Map()`) survives;
   - the total-domain initializer count (`HASHES.mapBy` / `PATHS.mapBy` /
     IDS-domain `.mapBy`) is at least the number of sites the landed slice
     rewrote: init + DbLoss + the drain/apply sites;
   - the file-wide floors for `PATHS.mapBy` (14), IDS-domain `.mapBy` (6)
     and `HASHES.mapBy` (5) hold.  Floors are lower bounds: later slices may
     add sites.
d. `pure val G = <int>` in the proof entry parses as an int and agrees with a
   literal bound declaration in the core (`G` or `GEN_LIMIT`), if the core has
   one.  The core cannot spell the bound `G` itself: the proof entry declares
   `G` and imports the core with `.*`, and Quint rejects conflicting
   definitions.
e. The core still declares `module pipeline_merged`, the copy-identity marker
   that ties it to the source model.
f. Slice-7 rewrite invariants:
   - every `files' =` write is one of the key-domain-preserving forms
     (stutter, `.set`/`.setBy` overwrite, total initializer, per-pair
     `.set` chain), and no `files.exclude`/`files.filter` removal exists;
   - every `generations' =` write is one of the saturating forms and contains
     `satSucc(` except the all-zero total resets; no `generations.get(X) + 1`
     bump remains; `satSucc` is declared once, is saturating at `GEN_LIMIT`,
     and is called;
   - every `intake' =` write is a stutter, union, filter-removal, or the empty
     set, and every generation field inside an intake element constructor is
     `satSucc(...)` or `0`;
   - the proof entry keeps the per-field intake universe conjunct (§2.1).
g. The `nextAssetId` counter is gone (zero occurrences in the core) and
   `nextFreeId` is declared once, reads the absent sentinel, and is used.

Exit status: 0 when every check passes, 1 when a check fails, 2 on I/O or
usage errors.  `--verbose` prints the per-check report; the default prints
only failures.  `--core` / `--proof` point the audit at copies of the files
(the default is this repository's inductive lane).

Usage:
    utils/quint/inductive/tools/audit.py
    utils/quint/inductive/tools/audit.py --verbose
    utils/quint/inductive/tools/audit.py --core /tmp/core.qnt --proof /tmp/proof.qnt
"""
import argparse
import re
import sys
from pathlib import Path

LANE_DIR = Path(__file__).resolve().parents[1]
DEFAULT_CORE = LANE_DIR / "inductive-core.qnt"
DEFAULT_PROOF = LANE_DIR / "inductive-proof.qnt"

# Tables whose slices have landed, in slice order.  files/generations are
# already total maps in the source model; slice 7a only verified that, slice
# 7b added value saturation.
LANDED_TABLES = (
    "thumbPresent",
    "sweepBatch",
    "assetByPath",
    "assetById",
    "metadataPayload",
    "dupeIndex",
    "files",
    "generations",
)

# Initializer key domain per landed table and the minimum site count the
# slice rewrote, from the codemod scripts:
#   thumbPresent: init, LoseThumbs, DbLoss              -> 3 (HASHES)
#   sweepBatch:   init, ProcessCrash, DbLoss,
#                 SweepEnumerate, SweepRecheck, Apply   -> 6 (PATHS)
#   assetByPath:  init, DbLoss, SweepRoot, Apply        -> 4 (PATHS)
#   assetById:    init, DbLoss, SweepRemoveBatchApply   -> 3 (IDS)
#   metadataPayload: init, DbLoss, SweepRemoveBatchApply -> 3 (IDS)
#   dupeIndex:    init, DbLoss                          -> 2 (HASHES)
#   files:        init (DbLoss stutters)                -> 1 (PATHS)
#   generations:  init, DbLoss                          -> 2 (PATHS)
MIN_TABLE_INITIALIZERS = {
    "thumbPresent": ("HASHES", 3),
    "sweepBatch": ("PATHS", 6),
    "assetByPath": ("PATHS", 4),
    "assetById": ("IDS", 3),
    "metadataPayload": ("IDS", 3),
    "dupeIndex": ("HASHES", 2),
    "files": ("PATHS", 1),
    "generations": ("PATHS", 2),
}
MIN_PATHS_MAPBY = 14
MIN_IDS_MAPBY = 6
MIN_HASHES_MAPBY = 5

# IDS is either named in the core or written out where the table is used.
IDS_DOMAIN = r"(?:IDS|0\.to\(PATHS\.size\(\)\s*\+\s*1\))"

# Declared in the core and unreferenced there at the baseline commit.  Kept as
# the aspect observer that traceability.json names; not a slice artifact.
BASELINE_UNUSED_DEFS = {"ThumbPresent"}

DEF_RE = re.compile(r"^\s*(?:pure\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)", re.M)
MODULE_RE = re.compile(r"^module\s+pipeline_merged\s*\{", re.M)

# Slice 7a: files stays total over PATHS.  A write may only stutter, overwrite
# an existing key (`set`/`setBy`), rebuild the whole domain, or set both sides
# of a declared pair.  Removal is written as `present: false`, never as a key
# drop, so no removal form appears.
FILES_WRITE_FORMS = (
    "files.set(",
    "files.setBy(",
    "PATHS.mapBy(",
    "SUBTREE_PAIRS.foldl(files,",
)

# Slice 7b: generations stays total over PATHS with values 0..GEN_LIMIT.  Every
# non-reset write contains a satSucc bump; the resets rebuild the domain at 0.
GENERATIONS_WRITE_FORMS = (
    "PATHS.mapBy(",
    "generations.set(",
    "SUBTREE_PAIRS.foldl(generations,",
)
GENERATIONS_BUMP_RE = r"generations\.get\([^()]*\)\s*\+\s*1"
ALL_ZERO_GENERATIONS_RE = r"PATHS\.mapBy\(_\s*=>\s*0\)"

# The seven fields of the §2.1 intake universe, as the proof entry must bound
# them (whitespace is normalised before matching).
INTAKE_UNIVERSE_BOUNDS = (
    "intake.forall(i =>",
    "i.path.in(PATHS)",
    "i.generation.in(0.to(G))",
    'i.kind.in(Set("watcher", "remove", "scan"))',
    "i.mtime.in(SMALL)",
    "i.len.in(SMALL)",
    "i.sidecar.in(SMALL)",
    "i.assetHash.in(HASHES)",
)


class Check:
    def __init__(self, name, ok, detail, lines=()):
        self.name = name
        self.ok = ok
        self.detail = detail
        self.lines = list(lines)


def strip_comments(text):
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def line_of(text, offset):
    return text.count("\n", 0, offset) + 1


def count_matches(pattern, text):
    return len(re.findall(pattern, text))


def find_sites(pattern, text):
    """Return (line_number, line_text) for each regex match."""
    hits = []
    for m in re.finditer(pattern, text):
        start = text.rfind("\n", 0, m.start()) + 1
        end = text.find("\n", m.start())
        end = len(text) if end == -1 else end
        hits.append((line_of(text, m.start()), text[start:end].strip()))
    return hits


def assignment_expr(text, start):
    """Return the right-hand side of an assignment beginning at `start`.

    The expression runs to the first comma outside brackets and strings, so
    multiline expressions and folded seeds are captured whole.
    """
    i = text.find("=", start)
    if i == -1:
        return ""
    i += 1
    depth = 0
    in_str = False
    while i < len(text):
        c = text[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                in_str = False
        elif c == '"':
            in_str = True
        elif c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth < 0:
                return text[start:i]
        elif c == "," and depth == 0:
            return text[start:i]
        i += 1
    return text[start:i]


def table_initializers(text, table):
    """Return the RHS of every `T' = ...` assignment in the file."""
    exprs = []
    for m in re.finditer(r"\b%s'\s*=" % re.escape(table), text):
        exprs.append((line_of(text, m.start()), assignment_expr(text, m.start())))
    return exprs


def check_keys_gone(table, core_text):
    name = "a.%s" % table
    sites = []
    for m in re.finditer(r"\b%s\.keys\(\)" % re.escape(table), core_text):
        # A total map may still assert its key domain; only a keys() read used
        # for iteration/projection is a stale sparse view.
        if re.match(r"\s*(==|!=)", core_text[m.end():]):
            continue
        sites.append((line_of(core_text, m.start()), core_text[
            core_text.rfind("\n", 0, m.start()) + 1:core_text.find("\n", m.start())
        ].strip()))
    if not sites:
        return Check(name, True, "0 `%s.keys()` iteration sites (expected 0)" % table)
    return Check(
        name,
        False,
        "%d `%s.keys()` iteration site(s) remain; the table is total over a "
        "constant domain, so presence must read the sentinel (slice rewrite "
        "incomplete)" % (len(sites), table),
        sites,
    )


def check_no_sparse_init(table, core_text):
    name = "c.sparse_init.%s" % table
    sites = find_sites(r"\b%s'?\s*=\s*Map\(\)" % re.escape(table), core_text)
    if not sites:
        return Check(name, True, "no `%s = Map()` sparse initializer" % table)
    return Check(
        name,
        False,
        "%d sparse `%s = Map()` initializer(s); a total map needs its full key "
        "domain (init and DbLoss)" % (len(sites), table),
        sites,
    )


def check_table_initializers(table, core_text):
    name = "c.initials.%s" % table
    domain, minimum = MIN_TABLE_INITIALIZERS[table]
    if domain == "IDS":
        pattern = r"\b%s\.mapBy\(" % IDS_DOMAIN
    else:
        pattern = r"\b%s\.mapBy\(" % domain
    sites = [
        (line, expr)
        for line, expr in table_initializers(core_text, table)
        if re.search(pattern, expr)
    ]
    detail = "%d total-domain `%s' = %s.mapBy(` initializer(s), floor %d" % (
        len(sites),
        table,
        domain,
        minimum,
    )
    return Check(name, len(sites) >= minimum, detail)


def check_mapby_floor(core_text):
    paths_count = count_matches(r"\bPATHS\.mapBy\(", core_text)
    ids_count = count_matches(r"\b%s\.mapBy\(" % IDS_DOMAIN, core_text)
    hashes_count = count_matches(r"\bHASHES\.mapBy\(", core_text)
    checks = [
        Check(
            "c.paths_mapby",
            paths_count >= MIN_PATHS_MAPBY,
            "%d `PATHS.mapBy(` initializer(s), floor %d"
            % (paths_count, MIN_PATHS_MAPBY),
        ),
        Check(
            "c.ids_mapby",
            ids_count >= MIN_IDS_MAPBY,
            "%d IDS-domain `.mapBy(` initializer(s), floor %d"
            % (ids_count, MIN_IDS_MAPBY),
        ),
        Check(
            "c.hashes_mapby",
            hashes_count >= MIN_HASHES_MAPBY,
            "%d `HASHES.mapBy(` initializer(s), floor %d"
            % (hashes_count, MIN_HASHES_MAPBY),
        ),
    ]
    return checks


def rhs_of(expr):
    """Return the right-hand side of an assignment expression including its
    left-hand side (table_initializers returns `T' = ...`)."""
    return expr.partition("=")[2].strip()


def read_balanced_text(text, open_idx):
    """Return the index of the ')' matching the '(' at open_idx, or -1."""
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return i
    return -1


def check_write_forms(table, forms, core_text):
    name = "f.forms.%s" % table
    bad = []
    for line, expr in table_initializers(core_text, table):
        stripped = rhs_of(expr)
        if stripped == table or stripped.startswith(forms):
            continue
        bad.append((line, stripped[:110]))
    if bad:
        return Check(
            name,
            False,
            "%d `%s' =` write(s) outside the total-domain forms (%s stutter or %s)"
            % (len(bad), table, table, ", ".join(forms)),
            bad,
        )
    return Check(
        name,
        True,
        "every `%s' =` write is the stutter or one of: %s" % (table, ", ".join(forms)),
    )


def check_no_removal_sites(table, core_text):
    name = "f.removals.%s" % table
    sites = find_sites(r"\b%s\.(?:exclude|filter)\(" % re.escape(table), core_text)
    if not sites:
        return Check(name, True, "0 `%s.exclude/filter` key-drop sites" % table)
    return Check(
        name,
        False,
        "%d `%s.exclude/filter` site(s); a total map encodes removal as a "
        "sentinel overwrite, not a key drop" % (len(sites), table),
        sites,
    )


def check_generations_saturating(core_text):
    name = "f.generations_bumps"
    bumps = find_sites(GENERATIONS_BUMP_RE, core_text)
    if bumps:
        return Check(
            name,
            False,
            "%d `generations.get(X) + 1` bump(s) remain; every bump must go "
            "through satSucc" % len(bumps),
            bumps,
        )

    unbounded = []
    for line, expr in table_initializers(core_text, "generations"):
        stripped = rhs_of(expr)
        if stripped == "generations":
            continue
        if re.fullmatch(ALL_ZERO_GENERATIONS_RE, stripped):
            continue
        if "satSucc(" not in stripped:
            unbounded.append((line, stripped[:110]))
    if unbounded:
        return Check(
            name,
            False,
            "%d `generations' =` write(s) neither all-zero reset nor "
            "satSucc-bounded" % len(unbounded),
            unbounded,
        )
    return Check(
        name,
        True,
        "0 residual bump(s); every non-reset generations write goes through satSucc",
    )


def check_satSucc(core_text):
    name = "f.satSucc"
    code = strip_comments(core_text)
    decls = re.findall(r"(?m)^\s*pure def satSucc\(g: int\): int\s*=\s*(.+)$", code)
    if len(decls) != 1:
        return Check(
            name,
            False,
            "%d `pure def satSucc(g: int): int` declaration(s), expected 1"
            % len(decls),
        )
    body = decls[0].strip()
    if not re.fullmatch(r"if\s*\(g\s*<\s*GEN_LIMIT\)\s*g\s*\+\s*1\s*else\s*g", body):
        return Check(
            name, False, "satSucc body is not the saturating form: %s" % body
        )
    uses = len(re.findall(r"\bsatSucc\(", code)) - 1
    if uses < 1:
        return Check(name, False, "satSucc is declared but never called")
    return Check(
        name,
        True,
        "satSucc declared once, saturating at GEN_LIMIT, %d call site(s)" % uses,
    )


def check_intake_sites(core_text):
    name = "g.intake_sites"
    counts = {}
    for m in re.finditer(r"intake' = ", core_text):
        rest = core_text[m.end():]
        if rest.startswith("intake,"):
            form = "stutter"
        elif rest.startswith("intake.union("):
            form = "union"
        elif rest.startswith("intake.filter("):
            form = "filter"
        elif rest.startswith("Set()"):
            form = "empty"
        else:
            return Check(
                name,
                False,
                "`intake' =` write outside the set-preserving forms at line %d: %s"
                % (line_of(core_text, m.start()), rest[:80]),
                [(line_of(core_text, m.start()), rest[:80])],
            )
        counts[form] = counts.get(form, 0) + 1
    return Check(
        name,
        True,
        "intake writes: %s"
        % ", ".join("%s=%d" % (k, v) for k, v in sorted(counts.items())),
    )


def check_intake_generation(core_text):
    name = "g.intake_generation"
    bad = []
    unions = 0
    for m in re.finditer(r"intake' = intake\.union\(", core_text):
        open_idx = m.end() - 1
        close_idx = read_balanced_text(core_text, open_idx)
        if close_idx == -1:
            return Check(name, False, "unbalanced intake.union( at line %d" % line_of(core_text, m.start()))
        unions += 1
        expr = core_text[open_idx + 1:close_idx]
        for fm in re.finditer(r"generation:\s*", expr):
            rest = expr[fm.end():]
            if rest.startswith("satSucc(") or re.match(r"0\b", rest):
                continue
            bad.append(
                (line_of(core_text, open_idx + 1 + fm.end()), rest[:70])
            )
    if bad:
        return Check(
            name,
            False,
            "%d intake generation field(s) not satSucc/0 bounded" % len(bad),
            bad,
        )
    return Check(
        name,
        True,
        "%d intake union site(s); every generation field is satSucc(...) or 0"
        % unions,
    )


def check_intake_universe(proof_text):
    name = "g.intake_universe"
    proof_code = " ".join(strip_comments(proof_text).split())
    missing = [p for p in INTAKE_UNIVERSE_BOUNDS if p not in proof_code]
    if missing:
        return Check(
            name,
            False,
            "proof entry intake universe conjunct is missing: %s"
            % "; ".join(missing),
        )
    return Check(
        name,
        True,
        "proof entry bounds all 7 intake fields (path, generation, kind, mtime, "
        "len, sidecar, assetHash)",
    )


def check_nextassetid(core_text):
    name = "g.nextassetid"
    sites = find_sites(r"\bnextAssetId\b", core_text)
    if sites:
        return Check(
            name,
            False,
            "%d `nextAssetId` occurrence(s); the counter var is eliminated by "
            "the nextFreeId quotient" % len(sites),
            sites,
        )
    return Check(name, True, "0 `nextAssetId` occurrences (counter eliminated)")


def check_nextfreeid(core_text):
    name = "g.nextfreeid"
    code = strip_comments(core_text)
    decls = list(re.finditer(r"(?m)^[ \t]*def nextFreeId\s*:\s*int\s*=", code))
    if len(decls) != 1:
        return Check(
            name, False, "%d `def nextFreeId: int` declaration(s), expected 1" % len(decls)
        )
    end = code.find("\n\n", decls[0].start())
    body = code[decls[0].start():end if end != -1 else len(code)]
    missing = []
    if "1.to(PATHS.size() + 1)" not in body:
        missing.append("candidate domain 1.to(PATHS.size() + 1)")
    if 'assetById.get(id).kind == "absent"' not in body:
        missing.append('absent-record test')
    if missing:
        return Check(
            name,
            False,
            "nextFreeId does not read %s" % ", ".join(missing),
        )
    uses = len(re.findall(r"\bnextFreeId\b", code)) - 1
    if uses < 1:
        return Check(name, False, "nextFreeId is declared but never read")
    return Check(
        name,
        True,
        "nextFreeId declared once, reads the absent sentinel, %d read site(s)" % uses,
    )


def check_defs(core_text):
    code = strip_comments(core_text)
    declared = {}
    for m in DEF_RE.finditer(core_text):
        line = line_of(core_text, m.start())
        declared.setdefault(m.group(1), []).append(line)

    unused = []
    for name, lines in declared.items():
        refs = len(re.findall(r"\b%s\b" % re.escape(name), code)) - len(lines)
        if refs <= 0:
            unused.append((name, lines[0]))

    unexpected = [(n, ln) for n, ln in unused if n not in BASELINE_UNUSED_DEFS]
    exempt = [(n, ln) for n, ln in unused if n in BASELINE_UNUSED_DEFS]
    if unexpected:
        detail = "%d declared def(s) referenced nowhere else: %s" % (
            len(unexpected),
            ", ".join("%s:%d" % (n, ln) for n, ln in unexpected),
        )
        return Check("b.defs", False, detail, [(ln, n) for n, ln in unexpected])

    detail = "%d declared def(s), all referenced" % len(declared)
    if exempt:
        detail += "; baseline-unused exempt: %s" % ", ".join(
            "%s:%d" % (n, ln) for n, ln in exempt
        )
    return Check("b.defs", True, detail)


def check_G(core_text, proof_text):
    code = strip_comments(core_text)
    proof_code = strip_comments(proof_text)
    decls = re.findall(
        r"^\s*pure\s+val\s+G\s*=\s*(-?\d+)\b", proof_code, flags=re.M
    )
    if not decls:
        return Check(
            "d.G", False, "no `pure val G = <int>` declaration in the proof entry"
        )
    if len(set(decls)) > 1:
        return Check(
            "d.G", False, "conflicting `pure val G` declarations: %s" % ", ".join(decls)
        )

    g_entry = int(decls[0])
    refs = len(re.findall(r"\bG\b", code))
    core_literals = re.findall(
        r"^\s*(?:pure\s+)?(?:val|const)\s+(G|GEN_LIMIT)\s*(?::\s*int)?\s*=\s*(-?\d+)\b",
        code,
        flags=re.M,
    )
    if core_literals:
        bad = [(name, v) for name, v in core_literals if int(v) != g_entry]
        if bad:
            return Check(
                "d.G",
                False,
                "proof G = %d but core declares %s"
                % (g_entry, ", ".join("%s = %s" % (n, v) for n, v in bad)),
            )
        return Check(
            "d.G",
            True,
            "G = %d, matches core declaration (%s)"
            % (g_entry, ", ".join(n for n, _ in core_literals)),
        )
    return Check(
        "d.G",
        True,
        "G = %d; core has %d G reference(s) and no literal bound declaration"
        % (g_entry, refs),
    )


def check_module(core_text):
    if MODULE_RE.search(core_text):
        return Check("e.module", True, "declares `module pipeline_merged`")
    return Check(
        "e.module",
        False,
        "`module pipeline_merged` declaration missing; the core is no longer a "
        "copy of the source model's module",
    )


def run_audit(core_path, proof_path):
    try:
        core_text = core_path.read_text(encoding="utf-8", errors="replace")
        proof_text = proof_path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        print("audit: cannot read input: %s" % exc, file=sys.stderr)
        raise SystemExit(2)

    checks = []
    for table in LANDED_TABLES:
        checks.append(check_keys_gone(table, core_text))
    checks.append(check_defs(core_text))
    for table in LANDED_TABLES:
        checks.append(check_no_sparse_init(table, core_text))
        checks.append(check_table_initializers(table, core_text))
    checks.extend(check_mapby_floor(core_text))
    checks.extend(
        [
            check_write_forms("files", FILES_WRITE_FORMS, core_text),
            check_no_removal_sites("files", core_text),
            check_write_forms("generations", GENERATIONS_WRITE_FORMS, core_text),
            check_generations_saturating(core_text),
            check_satSucc(core_text),
            check_intake_sites(core_text),
            check_intake_generation(core_text),
            check_intake_universe(proof_text),
            check_nextassetid(core_text),
            check_nextfreeid(core_text),
        ]
    )
    checks.append(check_G(core_text, proof_text))
    checks.append(check_module(core_text))
    return checks


def report(checks, core_path, proof_path, verbose):
    failures = [c for c in checks if not c.ok]
    if verbose:
        print("audit: core=%s" % core_path)
        print("audit: proof=%s" % proof_path)
        for check in checks:
            status = "PASS" if check.ok else "FAIL"
            print("%s %-22s %s" % (status, check.name, check.detail))
    else:
        for check in failures:
            if check.lines:
                for index, (line, text) in enumerate(check.lines):
                    reason = check.detail
                    if index:
                        reason = "additional offending site: %s" % text[:80]
                    print("%s:%d: FAIL %s: %s" % (core_path, line, check.name, reason))
            else:
                target = proof_path if check.name == "d.G" else core_path
                print("%s:0: FAIL %s: %s" % (target, check.name, check.detail))

    if failures:
        print("audit: FAIL (%d/%d checks failed)" % (len(failures), len(checks)))
        return 1
    print("audit: PASS (%d checks)" % len(checks))
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Audit the inductive core's landed table rewrites."
    )
    parser.add_argument("--core", type=Path, default=DEFAULT_CORE, help="core model path")
    parser.add_argument("--proof", type=Path, default=DEFAULT_PROOF, help="proof entry path")
    parser.add_argument("--verbose", action="store_true", help="print the per-check report")
    args = parser.parse_args(argv)

    checks = run_audit(args.core, args.proof)
    return report(checks, args.core, args.proof, args.verbose)


if __name__ == "__main__":
    sys.exit(main())
