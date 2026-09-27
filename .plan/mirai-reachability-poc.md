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

- 2026-09-26: Step 4 executed a second time against the real framework, since
  the first pass had inferred the tokio and Rocket behavior from stand-ins.
  Fixtures, the reproducible matrix and the findings are in `tools/mirai-poc/`;
  `tools/mirai-poc/run-shapes.sh` re-checks all 64 rows and exits non-zero on a
  mismatch. Status stays `in-progress`: step 5 (backend subset) and step 6
  (integration decision) are untouched.

  - **Gate mechanics.** `run-shapes.sh` asserts the exit code and the diagnostic
    count as separate columns, because phase 1 showed they are independent: all
    62 analysis rows exit 0, including the 32 that have findings, and the exit
    code is asserted rather than assumed. The other 2 rows assert the failure
    and the fix for the MSRV blocker. A third column, `LIVE`, asserts from `MIRAI_LOG=info`
    that MIRAI really re-analyzed the selected function, because a fresh cargo
    fingerprint otherwise produces an empty, successful, convincing run. Each
    MIRAI run also redirects `TMPDIR` at the scratch directory: rustc and MIRAI's
    summary store write temporary files under the system temp directory, and this
    machine's per-user `/tmp` quota surfaces as `Disk quota exceeded` or as an
    internal panic in `SummaryCache::create_summary_store_if_needed` with
    exit 101. `run.sh` had the same latent problem and now does the same.
  - **Route parameter: yes, unconstrained input.** A query primitive reaching a
    private helper and `Option::unwrap` is reported at `--diag=paranoid` as
    ``possible called `Option::unwrap()` on a `None` value``, with the helper as a
    related location (`h1`, and `r1` with Rocket's real `#[get]` in the path). The
    `TREE_SNAPSHOT.read_row(..)` singleton-method form, a request-derived divisor
    (`possible attempt to divide by zero`) and a request-derived index are
    reported the same way. At `--diag=verify` none of these appear, because a
    parameter is unconstrained and a possible panic is not a proof: a whole-crate
    `verify` run has 3 diagnostics, all of them certain panics.
  - **`spawn_blocking`: no, and the dead end is locatable.** With the real
    `tokio 1.53.1`, `unwrap`/`expect`/index sinks inside the closure, and a sink
    in a helper the closure calls, produce 0 diagnostics in the fixture and 23
    in total (`k1`-`k4`). `MIRAI_LOG=debug` shows MIRAI entering 228 bodies from
    the handler through `tokio::task::blocking::spawn_blocking` down to
    `blocking::pool::spawn_thread`, where the exact blocker is
    `the called function did not resolve to an implementation with a MIR body` at
    `tokio-1.53.1/src/runtime/blocking/pool.rs:463:55`, the
    `std::thread::Builder::new()` call that would create the worker thread. The
    closure is carried as a value the whole way and never called. The phase-1
    `thread::spawn` result is the same wall one level deeper. The discriminating
    pair is `h14`/`h15`: the same `FnOnce` hand-off defined in the analyzed crate
    **is** followed and the sink **is** reported, so the limitation is not
    closures, `move`, or the generic hand-off.
  - **Body shape: yes.** Rocket's `Json<T>` works under a real `#[post]`: a field
    read through `Deref` and a field used as an index are both reported (`r4`,
    `r5`), and `into_inner()` through a helper is reported in the stand-in (`h6`).
  - **Guard: not detectable, at any level.** `let _ = auth;` and `let _ = auth?;`
    are indistinguishable: the discarded/propagated pairs (`h10`/`h11` and
    `h19`/`h20`) agree in every column at both diagnostic levels, including when
    the sink behind the guard is a _certain_ panic. A discarded guard does not
    even cost a diagnostic, and unlike `spawn_blocking` the `?` neither produces
    an `incomplete analysis` warning nor suppresses the sink behind it. MIRAI has
    no notion of a request guard, so this is ordinary unused code, and step 5's
    "was the timestamp guard propagated" question is not answerable by MIRAI.
  - **`async` is a hard blind spot, and it is the largest one.** A certain panic
    on a locally constructed `None` inside an `async fn` is silent at `verify` and
    at `paranoid` (`h24`), while the same panic in a called closure is reported as
    certain (`h23`) and an uncalled closure is silent (`h22`). An `async fn` body
    is a coroutine body, entered only by polling a future, which MIRAI never
    does; MIRAI selects the function (`analyzing selected function ...`) and then
    evaluates nothing in it. `get_rows` is `pub async fn`, and so is every handler
    under `backend/src/router/{get,put,post}`.
  - **One phase-1 reading corrected.** `Option::expect` is still invisible
    (`h2`), but `Result::expect` is not (`h21`, `r3`): only
    `option::expect_failed` has an `assume_unreachable!()` contract, and
    `Result::expect` falls back to `result::unwrap_failed`, which panics. Since
    `TreeSnapshot::read_scrollbar` returns a `Result`, the `expect` that step 5
    looks for is visible, which widens what step 5 can ask.
  - **First blocker of the Rocket experiment, and its reduction.** rocket 0.5.1
    has a non-optional dependency on `time` and pulls in `encoding_rs`; their
    current versions declare `rust-version = 1.88` and MIRAI's pinned compiler is
    `rustc 1.86.0-nightly`, so `cargo` refuses to build the graph. Note that
    `cargo generate-lockfile` only annotates the resolution and exits 0; the hard
    error comes from compiling. The root `Cargo.lock` pins those same versions
    (`time 0.3.55`, `time-core 0.1.9`, `time-macros 0.2.32`, `encoding_rs
0.8.40`), so **the backend's own resolved dependency set cannot be compiled by
    MIRAI's pinned compiler** and a backend experiment must first pin
    `time 0.3.45`, `time-core 0.1.7`, `time-macros 0.2.25`, `encoding_rs 0.8.35`.
    `tools/mirai-poc/fixture-msrv/` is the reduction: two dependencies and one
    empty function, where `cargo check --lib` exits 101 without the pins and 0
    with them. Both halves are gated rows. The same check over the whole root
    lock is worse: reading `rust-version` from the local crates.io index cache,
    **18 of 480 registry packages require rustc newer than 1.86**, including
    `redb 4.3.0` (1.90), `image 0.25.10` (1.88), `jsonwebtoken 11.1.0` (1.88)
    and the `icu_*` stack (1.88), and 141 more have no declared `rust-version`,
    so 18 is a lower bound. That is index metadata rather than a build, so a
    `cargo build` on the pinned nightly is what would confirm it, but
    `nightly-2025-01-10` is a long way behind the backend's dependency graph and
    `redb` would need a major-version downgrade rather than a patch pin. A
    backend experiment is therefore either a real pinning project or a narrower
    crate that avoids `redb`, `image` and `jsonwebtoken`.
  - **Cost.** Per case: the dependency-free fixture is 0.33-0.90 s (median
    0.39 s); `fixture-rocket` is 0.59-0.90 s per handler, with the crate-wide row
    between 33 s and 51 s across runs because it also builds the 163-package
    rocket graph through the MIRAI driver (391 MB of target dir; `cargo-mirai` puts
    `--cfg mirai -Z always_encode_mir` in `RUSTFLAGS` for the whole graph). In
    `fixture-tokio` the split is the finding: a case that calls `spawn_blocking`
    costs 13-16 s and reports nothing, every other case 0.3 s. Whether the backend
    fits a nightly lane is untested: 483 packages in the root lock, 20.9k lines
    across 107 files, 64 routes, and MIRAI's own `--crate_analysis_timeout`
    default of 240 s. A whole-crate run is one invocation; per-handler
    `--single_func` runs would re-analyze the crate per handler, and would not
    reach the async bodies in any case.
  - **Not tested.** The whole-crate run on the rocket fixture emits one
    `incomplete analysis ...` diagnostic per registered route, at the
    `#[get(..)]` attribute line inside the generated `into_route` function, and
    analyzes 21 entry points (7 handlers, 7 generated `into_route` methods, the
    guard's `from_request`, two storage methods, serde-generated methods, the
    `TREE_SNAPSHOT` initializer). Whether that noise scales to 64 routes is
    unknown. A dependency fixture run also puts 12 of 23 diagnostics inside
    tokio's own source, so a span filter has to decide about dependency spans,
    not just `core`/`std`.

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
