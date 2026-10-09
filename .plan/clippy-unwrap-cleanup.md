---
status: done
type: chore
priority: medium
area: backend
---

Original premise: retire ~140 `clippy::unwrap_used` call sites across the
backend (set to `warn` in `Cargo.toml`, excluded from the `-D warnings`
precommit gate), prioritizing redb/filesystem call sites.

## Findings (2026-10-09, re-scope)

The original premise is stale: production library code has 0 `.unwrap()`
call sites — the redb/FS sites were already retired; remaining ones in
`rebuild.rs`/`asset_store.rs` live inside `#[cfg(test)]` modules. The
original priority also referenced `fs-db-consistency-reporting.md`,
which does not exist in `.plan/`.

Measured with `cargo clippy --all-targets`: 180 `unwrap_used` warnings —

- 2 in `backend/build.rs` (build script, the only non-test sites)
- 178 in test code (125 unit-test modules, 15 `src/tests/` support,
  38 `backend/tests/` integration)

Precommit/CI run `cargo clippy` without `--all-targets`, so test code is
not compiled during linting. Therefore the only blocker for dropping
`-A clippy::unwrap_used` from the justfile is `backend/build.rs`.

## Scope (agreed)

- Fix the 2 `backend/build.rs` unwraps (`expect()` with context).
- Remove the `-A clippy::unwrap_used` carve-out from `justfile`
  (`backend-check`, `utils-check`).
- Update the `backend/Cargo.toml` lint comment and the
  `docs/test-strategy.md` § "Tooling caveats" bullet.

Out of scope (follow-up): the 178 test-code unwraps and the remaining
all-target lint debt — this is the deferred scope noted in
`automated-quality-gates.md` ("substantial existing test debt").

## Done (2026-10-09)

- `backend/build.rs`: both scenario/selftest name sites now use
  `expect()` with context; `cargo clippy -- -D warnings` passes without
  the `-A clippy::unwrap_used` carve-out.
- `justfile`: `-A clippy::unwrap_used` removed from `backend-check` and
  `utils-check` (the utils one was a no-op — those packages never enable
  the lint).
- `backend/Cargo.toml` comment and `docs/test-strategy.md` § "Tooling
  caveats" updated to describe the enforced gate.
- Verified: `just check`, `cargo test --no-run`, `just backend-test`.
