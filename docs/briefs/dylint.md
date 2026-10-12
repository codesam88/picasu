# Dylint

Run custom Rust lints from dynamic libraries — Clippy's machinery with
a user-chosen lint set, without forking Clippy. The point is
project-owned lint collections: rules about _your_ disciplines that
upstream Clippy will never carry.

## Baseline (the obvious third)

Three pieces with one iron coupling rule: a lint cdylib implementing
passes over HIR with full type information; a driver (`rustc` wrapper)
that loads libraries and registers their lints; and the rule that a
library and the crate it checks must share the nightly toolchain, or
the runner rejects the library. Libraries are discovered via workspace
metadata (`[workspace.metadata.dylint]`, path or git entries with
`pattern` scoping) or CLI flags (`--git`/`--pattern` for surveys),
merged under `--all`. Lint crates stay out of the stable workspace or
stable `cargo` trips over `rustc_private`. Maintenance is a pinned
nightly plus a re-pin cadence, with GREEN/RED re-verified at each pin.

## Why dynamic libraries beat forking Clippy

The naive route to a custom lint is forking Clippy, and the Trail of
Bits post shows why it rots: a fork carries its own two binaries to
keep on `PATH` without colliding, and worse, every lint in the fork
must agree on one compiler version — so a new lint wanting a newer
`clippy_utils` forces upgrading the whole fork or pinning the new lint
to old utilities. Dylint's answer is structural: lints ship as dynamic
libraries, each built against whatever toolchain it needs, and the
runner builds per-toolchain drivers on the fly (cached in the home
directory). Libraries sharing a toolchain are grouped, so symbol
resolution, type checking, and trait solving run once for all of them
— two lints in two drivers would compile the target twice. The
practical consequences: a lint set can straddle toolchains during a
migration instead of flag day, and `clippy_utils` skew (the fight that
burns every first attempt at third-party example lints) is a property
of _someone else's_ library, never yours — provided your own lint
doesn't depend on it.

## Project-specific lints as a discipline

The post's central example is worth absorbing: Dylint's own
`env_literal` lint exists because the authors kept writing
`remove_var("RUSTUP_TOOLCHIAN")` — string literals where a constant
belongs. Clippy does the same to itself (`internal_lints`:
every-lint-has-a-pass, wrapper-functions-only, non-default
descriptions). The rule of thumb: _if you write code you do not like
and you can detect it with a lint, the lint belongs in the tree_ —
not in review comments, where it will be re-litigated forever. Our
three lints follow exactly this shape (guard propagation, denial
shape, decode agreement), and the per-lint folder READMEs follow the
Clippy documentation format below.

## Writing good lints

- **Early vs late is the first decision.** `EarlyLintPass` sees
  syntax before type checking; `LateLintPass` sees HIR plus
  `TypeckResults` and is the default for anything that reasons about
  types. Security-discipline lints are late passes essentially always.
- **Document in the Clippy format** (What it does / Why is this bad?
  / Example with violating vs fixed code). It renders into browsable
  docs and forces the author to state the defect, not just the
  pattern — our folder-README standard mirrors it deliberately.
- **Test with UI tests.** Positive and negative fixture files with
  blessed `.stderr` output; regenerate deliberately, commit the
  expectation. A lint without both directions is a guess — this is the
  static-analysis analogue of RED/GREEN.
- **Bootstrap from the pattern.** The `author` lint generates
  detection code from an annotated example, and HIR-dump attributes
  show the actual node shapes before you write the visitor. Use both
  before hand-rolling traversal; desugaring (`?` → `match`, generated
  shims) is invisible otherwise.
- **Handle expansions first.** Macro-generated and desugared code is
  invisible unless checked: span-origin tests (`from_expansion`
  family) belong at the top of the pass, or generated shims become
  false positives. Our AUTH-3 learned this the hard way (~116
  Rocket-generated `Forward` mentions correctly ignored only because
  the enclosing-impl check excludes them).
- **Match types in preference order**: diagnostic items, then lang
  items, then def-path matching last. Our lints currently match by def
  path (fail-closed with spelling fallback) — correct without
  `clippy_utils`, and the documented upgrade path when a diagnostic
  item exists.
- **Diagnostics read matter-of-fact**, identifiers in grave accents,
  help text suggesting the fix. Terse warnings get ignored; ignored
  lints get deleted.

## Drift is the maintenance model

Nothing in the stack promises stability — not `rustc_private`, not
`clippy_utils` (its breaking change mid-article in the Trail of Bits
post is the canonical anecdote). So: pin one nightly for lint runs,
keep the main build on stable, re-pin on a cadence driven by lint
releases, and treat every re-pin as a porting task with GREEN/RED
proofs, not a version bump. The check stays advisory until it earns
gating; lints that cry wolf in CI get ignored, then deleted.

## Alternatives and when they win

- **Forking Clippy** still wins if the goal is contributing upstream:
  new general-purpose lints belong there, with its review and UI-test
  infrastructure.
- **rust-marker** offers a stable lint API over compiler internals —
  no nightly at all — but is explicitly experimental, and unproven
  whether its AST abstraction exposes everything a resolved-type +
  typeck + parent-node lint needs. Escape hatch if nightly pinning
  becomes painful, not today's choice.

## Pointers

- Trail of Bits, _Write Rust lints without forking Clippy_ (2021) —
  read for the driver mechanics, the `clippy_utils` dilemma, and the
  project-specific-lint philosophy:
  <https://blog.trailofbits.com/2021/11/09/write-rust-lints-without-forking-clippy/>
- Dylint `docs/how_dylint_works.md` — read for library discovery
  order, the four library requirements (filename convention,
  `dylint_version`, `register_lints`, `rustc_driver` link), and the
  toolchain-applicability limitation:
  <https://github.com/trailofbits/dylint/blob/master/docs/how_dylint_works.md>
- Clippy book, _Adding a new lint_ — read for the full workflow
  (scaffolding, UI tests, registration, MSRV, conflicting lints):
  <https://github.com/rust-lang/rust-clippy/blob/master/book/src/development/adding_lints.md>
- Clippy book, _Common tools for writing lints_ — read for type
  matching, trait-impl checks, and macro/expansion handling:
  <https://github.com/rust-lang/rust-clippy/blob/master/book/src/development/common_tools_writing_lints.md>
- rustc-dev-guide (HIR, type checking, diagnostics chapters) + nightly
  `rustc_hir`/`rustc_middle` docs — read when the HIR shapes surprise
  you: <https://rustc-dev-guide.rust-lang.org/>
- EuroRust 2024 talk (slides + video linked from the dylint README) —
  read for the 20-minute architectural overview.
