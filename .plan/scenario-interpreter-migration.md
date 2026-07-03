---
status: done
type: chore
priority: high
area: testing
---

Replace the YAML-to-Rust codegen in `xtask/src/generator.rs` with a pure interpreter that lives in `urocissa/gallery-backend/src/tests/`. Eliminate the intermediate `scenarios_generated.rs` file and the `check-generated` drift guard.

**Current state**

`cargo xtask test-backend` reads `gallery-backend/tests/scenarios/*.yaml`, emits Rust test code into `gallery-backend/src/tests/scenarios_generated.rs`, then runs `cargo nextest` on it. The generated code calls fixture helpers from `xtask::fixtures::*` and urocissa test bootstrap.

Separately, `cargo xtask test-generator` reads `gallery-backend/tests/scenarios/selftest/*.yaml` (4 files with deliberately wrong assertions), wraps the generated body in `catch_unwind`, and asserts panic — validating the assertion pipeline itself. Output goes to `gallery-backend/src/tests/test_generator_generated.rs`.

**Goal**

An interpreter in urocissa's test tree that reads YAML scenarios and executes them directly against a Rocket `Client` — no intermediate file, no codegen compile step, coverage counts toward urocissa. xtask shrinks to the `plan` tool only.

**Architecture**

```
gallery-backend/src/tests/
├── bootstrap.rs              stays
├── fixtures/                 moved from xtask/src/fixtures/
├── scenario_interpreter.rs   new — reads YAML, executes in-process
├── scenarios_generated.rs    deleted
└── test_generator_generated.rs  deleted

xtask/src/
├── plan.rs                   stays
├── main.rs                   stays (drops test-backend, test-generator subcommands)
└── generator.rs              removed
└── fixtures/                 removed

gallery-backend/tests/scenarios/
├── *.yaml                   20 API scenarios
└── selftest/                 4 self-test scenarios (assertion machinery validation)
```

`cargo xtask test-backend` becomes `cargo test -p urocissa -- scenario_interpreter`.

**Design constraint**

The interpreter must use only externally documented interfaces: Rocket startup via `build_rocket_with_config()`, the public HTTP API (all `GET`/`POST`/`PUT`/`DELETE` routes), and the `IMAGE_HOME` filesystem directory. No direct redb access, no private module imports. This keeps E2E tests valid across internal storage changes.

**Negative test approach**

The 4 `gallery-backend/tests/scenarios/selftest/*.yaml` files test the assertion machinery by feeding deliberately wrong assertions and expecting panic. In the interpreter, this becomes a separate test function that:

- Reads each generator YAML
- Runs it through the interpreter's assertion logic
- Wraps in `catch_unwind` and asserts the interpreter panicked

These scenarios live alongside the interpreter or inline as a `#[test]` function within `scenario_interpreter.rs`. The `test_generator_generated.rs` file and the `xtask/test-generator` subcommand are deleted.

**Serialization guards (`INDEX_SERIAL_GUARD`, `PREFETCH_SERIAL_GUARD`)**

Currently each generated `#[test]` function holds these `Mutex` guards. The interpreter runs all scenarios sequentially within a single `#[test]`, so it holds the guards for its full duration. This is safe — scenarios already cannot run in parallel with each other in the interpreter model.

**Steps**

1. Move `xtask/src/fixtures/` to `gallery-backend/src/tests/fixtures/` — adjust imports, remove xtask dev-dep from urocissa
2. Write `scenario_interpreter.rs` in urocissa tests — reads YAML from workspace-relative path, boots Rocket `Client`, steps through given/when/then, asserts. Include negative test harness for the 4 generator scenarios
3. Validate against all 20 backend YAML scenarios + 4 generator YAML scenarios
4. Delete `xtask/src/generator.rs`, `xtask/src/fixtures/`, `gallery-backend/src/tests/scenarios_generated.rs`, `gallery-backend/src/tests/test_generator_generated.rs`
5. Update `justfile`: remove `test-backend`, `gen`, `check-generated`, `test-generator` recipes; add `test-backend → cargo test -p urocissa -- scenario_interpreter`
6. Update `gallery-backend/src/tests/mod.rs` — replace `mod scenarios_generated` and `mod test_generator_generated` with `mod scenario_interpreter`
7. Update `docs/test-strategy.md` to reflect the new architecture
8. Clean up `gallery-backend/Cargo.toml` — remove `xtask` dev-dependency

**Done notes** (June 2026):

- All 20 backend scenarios + 4 negative scenarios pass via `backend_api`
- Fixed `read_only_mode` config cleanup ordering bug (moved after when/then)
- xtask shrunk to `openapi-gen` and `plan` subcommands only
- `cargo test -p urocissa -- backend_api` is the replacement for the old codegen test suite
