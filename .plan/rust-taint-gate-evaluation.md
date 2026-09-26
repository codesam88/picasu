---
status: done
type: chore
priority: medium
area: testing
---

## Problem

A taint-tracking gate was wanted for the question "can client input reach a
panic construct?" — the failure class behind `GET /get/get-scroll-bar`
panicking on an unknown snapshot id and behind `get-rows` /
`get-scroll-bar` discarding their `GuardTimestamp`.

## Findings (2026-09-25)

**CodeQL cannot answer it for a Rocket app.** The taint framework exists for
Rust (`TaintTracking::Global`, documented path queries) and a query compiles and
runs against a real database in ~1 min. Two independent blockers:

1. _Attribute token trees are not extracted._ Verified on a real database:
   `Meta.getExpr()` is null for `#[get("/get/data?<start>&<end>")]` and for
   `#[utoipa::path(path = "/get/data", …)]`; no `StringLiteralExpr` exists for
   those strings; `routes![a, b]` yields a `MacroCall` with no modeled arguments.
   Confirmed by a maintainer in github/codeql#20771: _"we currently do not
   extract the token tree of attributes, so it is not possible to do this on the
   QL side."_ Proc-macro expansion _is_ performed, but only the expanded AST is
   queryable, with no link to the declared path. Including dependencies does not
   help: the missing data is in this crate's own attributes.
2. _Path resolution was removed from the Rust extractor_ (rust CHANGELOG), which
   deletes `getCanonicalPath()` — the API CodeQL's only hand-written
   web-framework model (`Poem.qll`) uses to recognise request parameters by type.
   A Rocket model in that style therefore does not compile against 2.23.0, and
   `Poem.qll` is effectively dead code.

**What CodeQL _is_ good for here:** plain AST facts, fast and reliable. A panic
inventory query (`.expect`/`.unwrap`/`panic!`/`unreachable!`/`todo!` plus
indexing) ran in 23s. Cross-checked against clippy's restriction lints on
production code only (`#[cfg(test)]` stripped, full paths): **155 sites both
agree, 0 CodeQL-only** — clippy is a strict superset for `src/`, its extra 14
being 8 in the separate `build.rs` crate and 6 attribution differences. Useful as
an independent validation of a clippy-based inventory.

**State of the art for Rust taint** (surveyed, none usable here): **Charon**
(MIR→LLBC, has a taint case study, ~80s translation per crate); **RustGuard**
(rustc-internal, 91.67% precision/recall, 14% compile overhead, needs nightly);
**mrustc**, **Mirai**, **Prusti** (can prove `panic!` unreachable, annotation
heavy), **Rudra** (unsafe patterns). IFC-as-library: **Cocoon → Filament**
(2026, no compiler changes, but requires labelling code). Dynamic:
**PanicKiller** (CCS'25, dynamic taint to get _past_ runtime safety checks in
fuzzers). Shallow AST: `rusty-taint-check` (syn-based, one hop, explicitly does
not cross closures/threads/module boundaries — which is the `spawn_blocking`
case). Every precise option needs nightly/rustc internals or a per-crate MIR
translation, and **none models Rocket**, whose request→parameter plumbing lives
in the dependency.

## Decision

No off-the-shelf tool provides a request-input→panic oracle for this app. The
check is therefore built in-repo on `syn`, which is already in the dependency
tree (via `rocket_codegen`/`utoipa-gen`) and can see the macro arguments no
other tool can.

`build/ast_scan.rs` (committed in `c859a51c`) is the foundation: it parses the
router sources, so route handlers, their verb attributes and their `routes![]`
membership are AST facts. The panic-site gate extends it, and
`build/route_path.rs` already holds the one shared Rocket→OpenAPI translation
that the parity gate and the build-time annotation check both use.

## Notes

Follow-up, when the AST gate exists: the CodeQL cross-check can be committed as
an on-demand or nightly `just` recipe, not a per-PR gate — the 744 MB CLI
download and ~3 min database build do not belong in the PR path. A maintainer
also documents excluding test code at extraction time
(`-O cargo_cfg_overrides=-test`, or
`CODEQL_EXTRACTOR_RUST_OPTION_CARGO_CFG_OVERRIDES=-test`); the CLI form did not
exclude `src/tests/` in testing here (155 app + 102 test sites), so the correct
incantation still needs pinning down.

## Progress

- 2026-09-26: **Gate built** as this evaluation decided: `backend/build/reachability.rs`
  (syn-based, sees the attribute token trees CodeQL cannot) with the registry
  gate in `backend/src/tests/reachability.rs` and `backend/reachability-registry.txt`.
  See `.plan/request-taint-reachability-gate.md` for results, the acid test and
  the recorded limits.
