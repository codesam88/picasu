---
status: open
type: feature
priority: medium
area: testing
---

Add **coverage reports** for both backend (Rust) and frontend (Vitest).
Ensure expected test coverage is met, identify gaps.

## Scope

1. Select a backend coverage tool. `cargo-llvm-cov` (via `cargo llvm-cov`) is the current de-facto standard — single command, instrumented compile, lcov/html output.
2. Add a `just coverage` (or `just cov`) recipe running `cargo llvm-cov --html` (or alongside `cargo nextest`).
3. Configure Vitest coverage (`vitest.config.ts` with `coverage: { provider: 'v8', ... }`) and add a `just frontend-coverage` recipe.
4. Add a combined `just coverage` that runs both and merges/summarizes.
5. Wire coverage into CI — either as a PR comment (Codecov annotator) or a CI step that fails if coverage drops below a threshold.
6. Optionally integrate with Codecov or Coveralls for trend tracking.

## Sequencing

- Step 1-3: get local coverage working first (tool install + just recipe).
- Step 5 depends on CI that runs tests — `expand-ci-configurations` is a prerequisite for meaningful coverage CI.
- Thresholds should be set after first measurement, not before — see what the baseline is, then decide.

## Key considerations

- `cargo-llvm-cov` is not a Cargo dependency — it's a cargo subcommand installed via `cargo install cargo-llvm-cov` (add to `just install-dev`).
- Instrumented builds are slower; keep coverage runs separate from the fast `just precommit` cycle.
- Rust test coverage of the E2E API scenarios (Rocket `Client` tests) is what exercises the route handlers — those are the high-value targets for coverage measurement, not just the inline unit tests.
- Frontend coverage is optional at first (only the lexer is tested) — primary value is backend.
- The `test-strategy.md` regression matrix could eventually be cross-referenced with coverage data to identify untested paths.
