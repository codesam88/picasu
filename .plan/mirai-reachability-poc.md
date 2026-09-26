---
status: in-progress
type: chore
priority: high
area: testing
---

## Goal

Evaluate whether MIRAI can provide a stronger request-input-to-panic analysis
for the Rocket backend than the source-level `syn` prototype. This is a bounded
PoC, not an immediate CI integration.

## Iterative Steps

1. **Toolchain compatibility.** Identify the maintained MIRAI distribution,
   required Rust toolchain, installation method, and whether the current
   backend can be analyzed without changing application code. Acceptance:
   `cargo mirai` or the equivalent wrapper runs on a minimal fixture and exits
   with a machine-readable result.
2. **Panic baseline.** Run MIRAI on a minimal fixture containing an input-driven
   `expect`, `unwrap`, bounds check, and panic macro. Confirm which cases it
   detects without annotations and record false positives/negatives.
3. **Taint modeling.** Add the smallest MIRAI-only source/sink annotations
   needed to model untrusted handler input and panic sinks. Use `cfg(mirai)` or
   an isolated fixture so production behavior and dependencies remain
   unchanged. Acceptance: a tainted value reaching `expect` fails, and a
   non-tainted configuration invariant does not.
4. **Rocket shape.** Test the exact shapes needed by Picasu: a handler-like
   function, a helper call, a `move` closure, and `spawn_blocking`. Acceptance:
   the analysis detects a request-derived value reaching a sink through the
   closure/helper path, or the limitation is demonstrated with a reproducible
   failing fixture.
5. **Backend subset.** Run the tool against a narrow backend target or a
   generated fixture based on `get_rows`, `get_scroll_bar`, and
   `read_scrollbar`. Do not change handlers. Acceptance:
   `timestamp -> read_scrollbar -> expect` and discarded guard behavior are
   either detected or explicitly proven unsupported.
6. **Integration decision.** Compare MIRAI output with the current `syn`
   registry and the existing endpoint scenarios. Choose one outcome:
   - adopt MIRAI as a scheduled/nightly gate;
   - retain it as an advisory diagnostic and keep the `syn` ratchet; or
   - reject it with documented evidence and a follow-up alternative.

## Constraints

- Do not alter production handler behavior or add MIRAI-only dependencies to
  the shipped binary.
- Pin external tool commits/toolchains in scripts or documentation if the PoC
  becomes repeatable.
- Every step must have an explicit command and observed result.
- Commit only bounded, verified steps; do not commit generated databases or
  tool caches.

## Progress

- 2026-09-26: Steps 1 to 4 executed. Evidence, fixtures and a re-runnable
  matrix are in `tools/mirai-poc/`; `tools/mirai-poc/run.sh` re-checks every
  claim below and exits non-zero on a mismatch. Status stays `in-progress`:
  step 5 (backend subset) and step 6 (integration decision) are untouched.

  - **Toolchain (step 1).** The maintained distribution is
    [endorlabs/MIRAI](https://github.com/endorlabs/MIRAI), not archived, with
    `main` at `aae0a56f91d9a0a62073bb86e7b2c9793cefc24b` (2025-03-04), identical
    to tag `v1.1.12`. Its own `rust-toolchain.toml` pins `nightly-2025-01-10`
    (`rustc 1.86.0-nightly (824759493 2025-01-09)`) with `rustc-dev` and
    `rust-src`, because MIRAI is a `rustc` driver linked against private
    compiler APIs. That nightly was already installed, so nothing was installed
    or pinned: the backend's `stable` 1.96.0 toolchain and global config are
    unchanged, and the fixtures are built under `/tmp/opencode/mirai-poc`.
    A caveat for step 6: `main` has had no commit since 2025-03-04, so the tool
    is effectively frozen.
  - **Panic baseline (step 2).** On a minimal fixture at `--diag=paranoid`,
    MIRAI reports a request-derived parameter reaching `Option::unwrap`,
    `Result::unwrap`, a slice index out of bounds, and does so through a private
    helper and through a `move` closure. A locally constructed `Some(7)` is
    proven safe, so the false-positive control is clean. `--diag=default` and
    `--diag=verify` report nothing at all for a function parameter, which is
    unconstrained: only `paranoid` reports a _possible_ panic. Two silent cases
    are modelling choices, not limits of this configuration: `Option::expect` is
    `assume_unreachable!()` in the tool's own contract, and a written `panic!`
    has no diagnostic at all. **Step 5 is written around `expect`, which MIRAI
    cannot see; that changes what the backend subset can prove.**
  - **Taint modeling (step 3).** MIRAI 1.1.12 has no untrusted notion. The only
    mechanism is the tag domain: `add_tag!` as the source,
    `precondition!(does_not_have_tag!(..))` as the sink. Request-derived input
    **must** be explicitly tagged, and the tag only survives when it is placed on
    a place the sink can still name: a handler parameter, or a struct field. A
    tag on a `let` copy of a request field is silently lost, even when the sink
    is in the same function. This is measured per case in
    `tools/mirai-poc/README.md`. Using it would mean `mirai-annotations` in the
    backend and annotations on generated Rocket request structs, which the
    constraints in this plan rule out as a first move.
  - **Rocket shape (step 4).** The first unsupported shape is the thread
    hand-off: the value moved into a `thread::spawn` closure is never analyzed,
    and the run degrades to `incomplete analysis ... function without a MIR body`
    plus standard-library noise. The helper and `move`-closure paths are
    supported. The tokio `spawn_blocking` form is _inferred_ to fail the same way
    from the fixture and still needs its own run.
  - **Gate mechanics, found while executing the above.** Only the root crate's
    own bodies are analyzed: a panic inside a dependency is reported at the root
    call site that reaches it, and an unreached dependency panic is invisible.
    Every MIRAI diagnostic is a warning, so the exit code is 0 in all 23 cases
    and neither `RUSTFLAGS="-D warnings"` nor `-D warnings` in `MIRAI_FLAGS`
    promotes them. The one exit-code mechanism MIRAI has is the
    `//~ expected error` harness, and it is only reachable from MIRAI's own
    integration test runner, not from `cargo mirai`. A gate would have to scrape
    `[MIRAI]` out of the output and filter the `core`/`std` spans that
    `--diag=paranoid` also emits.

  Not run: step 5 (no backend file has been touched) and step 6.

- 2026-09-26: Created `mirai-reachability-poc` from `5083c5fb`, before the
  source-level `syn` route-discovery PoC. The prior analysis established that
  MIRAI is the most promising immediate MIR-based candidate, but compatibility
  and Rocket source modeling must be tested rather than assumed.
