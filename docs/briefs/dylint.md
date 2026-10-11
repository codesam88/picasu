# Dylint

Run custom Rust lints from dynamic libraries — Clippy's machinery with
a user-chosen lint set, without forking Clippy. The point is
project-owned lint collections: rules about _your_ disciplines (guard
propagation, annotation shape) that upstream Clippy will never carry.
`cargo-dylint` is the runner; lint crates are ordinary Rust crates
built as cdylibs that the runner loads and registers.

## Mental model

Three pieces with one iron coupling rule:

- **Lint library** — a cdylib implementing lint passes over the
  compiler's high-level IR (HIR) with full type information. This is
  what makes Dylint rules precise where text-level checkers guess:
  resolved types, cross-item lookup, desugared control flow.
- **Driver** — the `rustc` wrapper that loads the libraries and runs
  the check. `cargo-dylint` owns driver mechanics (building libs,
  filename conventions, environment setup); hand-rolled drivers are a
  spike-only expedient.
- **The coupling rule** — a lint library and the crate it checks must
  be compiled with the _same_ nightly toolchain, or the runner rejects
  the library. Everything about Dylint maintenance flows from this:
  the nightly pin, the re-pin cadence, why example lint sets can pull
  a second nightly.

Libraries are discovered two ways: workspace metadata in `Cargo.toml`
(`[workspace.metadata.dylint] libraries`, path or git entries, with
glob `pattern` scoping to subdirectories) for the standing set, and
CLI flags (`--git`/`--pattern`) for surveys. Both merge under `--all`.
Small libraries can live beside the code they check; they must not be
workspace members of a stable build, or stable `cargo` trips over
`rustc_private` code.

## Writing good lints

- **Implement `LateLintPass`.** Match on HIR nodes with the typecheck
  results as arbiter, never on syntax alone: spellings lie, resolved
  types don't. A same-named lookalike that resolves elsewhere must not
  match.
- **Fail closed.** Flag unless the code affirmatively demonstrates the
  discipline (handled, propagated, consumed) — zero uses of a guard
  binding is a violation, not an absence of evidence. Enumerate the
  neutral cases explicitly (compiler-generated moves, aliasing) so the
  default stays "warn".
- **Allow the escape hatch.** Standard `#[allow(...)]` works per lint;
  project-wide suppression belongs in reviewable config, not in the
  lint's blind spots. Known detection gaps get documented in the lint
  crate's README, not silently accepted.
- **Test like the compiler.** Dylint ships UI-test support on the
  `compiletest_rs` pattern: positive and negative cases as fixture
  files with expected warnings. A lint without both is a guess.
- **Borrow, don't depend, on `clippy_utils`.** The Clippy utility
  collection is public and useful, but carries the same no-stability
  guarantee as `rustc` itself — pin it to the nightly or it becomes
  the thing that breaks the re-pin.

## Maintenance reality

`rustc_private` has no stability promise, so every nightly bump can
move the API under the lints. The working policy: one pinned nightly
for lint runs only (the main build stays stable), re-pinned on a
cadence driven by lint-crate releases, with GREEN (clean tree, zero
findings) and RED (synthetic violation warns exactly at its site,
tree restored) re-verified at each pin. The check stays advisory until
a deliberate gating decision — lints that cry wolf in CI get ignored,
then deleted.

## Workflow notes

- **Conditional compilation.** The runner passes
  `--cfg=dylint_lib="<library>"` per loaded library, so code can
  `cfg_attr`-allow a lint only under Dylint and avoid unknown-lint
  noise otherwise; the `unexpected_cfgs` companion config keeps modern
  rustc quiet about the custom cfg.
- **Editor integration.** rust-analyzer can drive its checks through
  `cargo dylint` via the check-override command, with `rustc_private`
  source discovery for work inside lint crates.
- **CI caching.** Rebuilds dominate cost: cache the toolchain, the
  built drivers, and the lint build products alongside the usual cargo
  caches.
- **Upstream example sets.** The dylint repo ships general,
  experimental, and restriction example lints runnable by git+pattern.
  They survey well (one command, no vendoring) but each set tracks its
  own nightly — adopting one permanently can double the toolchain
  surface for findings of uneven value. Survey, triage, and only keep
  what earns its pin.

## Sources

Trail of Bits dylint repository and its README (workspace metadata,
configurable libraries, conditional compilation, CI caching),
the rustc development guide and HIR/`rustc_middle` documentation for
lint authoring, the Clippy authoring book for lint design. Re-verify
against current sources when starting new work; record the outcome in
the using plan.
