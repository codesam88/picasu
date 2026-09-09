---
status: idea
type: chore
priority: medium
area: testing
---

# Unified Backend Test Harness

## Status

### Current Test Infrastructure

| Layer                    | What it is                             | Runner                            | Process model                                     | Can test watcher             | Can test tasks             | Can perf-test |
| ------------------------ | -------------------------------------- | --------------------------------- | ------------------------------------------------- | ---------------------------- | -------------------------- | ------------- |
| **L0: Unit**             | `#[cfg(test)]` modules in src/         | `cargo test`                      | Pure functions, no I/O                            | No                           | No                         | No            |
| **L1: YAML scenarios**   | 67 declarative backend scenarios       | `cargo test` + `build.rs` codegen | In-process `rocket::local::blocking::Client`      | No (simulated via API)       | Partial (API trigger only) | No            |
| **L2: Rust integration** | 3 `.rs` files in `backend/tests/`      | `cargo test`                      | In-process Client + direct AppConfig manipulation | Partial (toggle only)        | No                         | No            |
| **L3: Playwright E2E**   | 24 YAML scenarios + 1 handwritten spec | `npx playwright test`             | Real `picasu` binary as child process             | Partial (config toggle only) | Partial (API trigger only) | No            |
| **L4: Vitest**           | 2 unit test files                      | `npx vitest`                      | Node process, no backend                          | No                           | No                         | No            |

### What's Missing

1. **Watcher behavior** — No test places a file on disk and verifies it gets auto-indexed by the filesystem watcher. Backend tests simulate via `POST /post/index/album`. Playwright tests toggle the config flag but never test the inotify→index pipeline.

2. **Background task execution** — Dedup (`DeduplicateTask`), flush (`FlushTreeTask`), expire check, and `UpdateTreeTask` are never exercised by any test. Backend tests trigger indexing via API but the task coordinator's batch/flush behavior is untested.

3. **Concurrent operations** — Backend scenarios are serialized via `TEST_SERIAL_GUARD`. No test verifies race conditions, concurrent uploads, or simultaneous API calls against the same record.

4. **Performance measurement** — No latency measurement, no load generation, no throughput benchmarking exists anywhere.

5. **Duplicated setup logic** — `given`/`when`/`then` DSL is implemented twice: once in Rust (`backend_api.rs`, ~400 lines) and once in TypeScript (`executeGiven.ts` + `interpreter.ts`, ~600 lines). Same concepts, different syntax, different capabilities.

### Root Causes

- **In-process limitation**: `rocket::local::blocking::Client` dispatches requests synchronously without starting background services. The watcher, expire loop, and batch coordinator are never started.
- **Global statics**: `IS_WATCHING`, `WATCHER_HANDLE`, `DEBOUNCE_POOL` are process-wide `LazyLock` statics. No way to create isolated watcher instances for parallel testing.
- **No child-process backend tests**: All Rust tests use in-process Client. The only code that spawns the real binary is `backendLauncher.ts` (TypeScript).
- **Two separate DSL implementations**: Backend and Playwright each implement their own YAML interpreter with overlapping but divergent `given`/`when`/`then` item types.

## Goals

### Primary

1. **Test watcher behavior end-to-end**: Place a file on disk → watcher detects → auto-indexes → visible via API.
2. **Test background task execution**: Verify dedup, flush, and expire tasks complete correctly via API assertions.
3. **Unify the test DSL**: One YAML schema that both the Rust runner and Playwright runner consume. Backend setup (`given`) and HTTP assertions (`when`/`then`) written once.

### Secondary

4. **Enable performance testing**: Measure API response latency under load, observe index pipeline throughput.
5. **Enable concurrent operation testing**: Verify correct behavior when multiple clients interact with the same records.
6. **Reduce maintenance burden**: One DSL implementation instead of two.

## Proposed Solutions

### Solution A: Rust Child-Process Harness + Unified DSL

Build a Rust library (`picasu-test`) that spawns the real `picasu` binary, waits for readiness, and executes `given`/`when`/`then` blocks via HTTP. Expose as both a library (for Rust tests) and a CLI (for Playwright).

```
picasu-test (Rust lib + CLI)
├── BackendProcess     — spawn binary, wait-ready, SIGTERM teardown
├── TestDataBuilder    — generate images (snapfab), place files, auth, index, discover
├── HttpClient         — reqwest wrapper with auth helpers
├── ScenarioRunner     — parse YAML, execute given/when/then
└── Assertions         — file checks, API response checks, prefetch/locate
         │
         ├── Rust tests: use as library (replaces backend_api.rs scenario runner)
         └── Playwright: call as CLI (replaces backendLauncher.ts + executeGiven.ts)
```

**Pros**: Single implementation, fastest possible Rust-native execution, enables perf testing, enables concurrent scenarios (each gets its own backend process).
**Cons**: Significant refactoring of existing 67 YAML scenarios (syntax changes), Playwright integration requires CLI bridge, new dependency for backend tests.

### Solution B: Extend Backend Launcher for Playwright-Only

Keep the existing in-process backend tests as-is. Build the watcher/task/perf capabilities only in the Playwright layer by extending `backendLauncher.ts` with:

- A `photo_raw` + polling `then` assertion for watcher testing
- A `wait_for_task` assertion for background task completion
- Timing instrumentation for perf measurement

**Pros**: Minimal disruption to existing tests, Playwright already spawns the real binary, fastest path to watcher coverage.
**Cons**: TypeScript-only, can't perf-test from Rust, doesn't unify the DSL, Playwright is slower than Rust for backend-only scenarios.

### Solution C: Hybrid — Rust Harness for Backend, Playwright Extends It

Build the Rust harness (Solution A) for backend-only scenarios. Playwright keeps its own `interpreter.ts` for browser steps but delegates `given` and backend `when`/`then` to the Rust harness CLI. The Playwright interpreter becomes a thin browser-interaction layer on top of the Rust backend harness.

**Pros**: Single source of truth for backend setup/assertions, Playwright focuses on browser interaction, enables perf testing from Rust, clean separation of concerns.
**Cons**: Most complex to implement, requires coordination between two test systems, Playwright scenarios need refactoring to use the CLI for backend steps.

## Implementation Plan (Solution C — Recommended)

### Phase 1: Rust Harness Core

1. **`picasu-test` crate** in `backend/tests/harness/`:
   - `BackendProcess`: spawn binary with env vars, poll health endpoint, SIGTERM/SIGKILL teardown
   - `HttpClient`: reqwest wrapper with JWT auth, retry, timeout
   - `TestDataBuilder`: `snapfab` integration, file placement, config mutation

2. **Scenario runner**:
   - Parse YAML (reuse `serde_yaml`)
   - Execute `given` items: `photo`, `dir_album`, `raw_file`, `remove`, `config`, `photo_raw`
   - Execute `when` items: `call`, `upload`, `wait_index`, `wait_watcher`
   - Execute `then` assertions: `response.status`, `response.json.*`, `file_exists`, `file_absent`, `serve_image_ok`

3. **Migrate existing YAML scenarios**:
   - Adapt 67 backend YAML files to the new DSL (mostly `given`/`when`/`then` syntax alignment)
   - Run both old and new runners in parallel during transition
   - Remove old runner once all scenarios pass

### Phase 2: Watcher + Task Testing

4. **Watcher test support**:
   - `wait_watcher` `when` item: polls `/get/get-data` until a file appears, with configurable timeout
   - New scenario: place file via `photo_raw` → `wait_watcher` → assert via `response.json.*`

5. **Background task assertions**:
   - `wait_task` `when` item: polls `/get/index/status` or specific task completion
   - New scenarios for dedup (upload identical content → verify single record)

### Phase 3: Performance Testing

6. **Latency measurement**:
   - `measure` `when` item: wraps HTTP call, records response time
   - `then` assertion: `latency_ms < <threshold>`
   - Configurable warmup iterations

7. **Load testing**:
   - `parallel` block: spawn N concurrent requests
   - Aggregate latency statistics (p50, p95, p99)

### Phase 4: Playwright Integration

8. **CLI interface** for `picasu-test`:
   - `picasu-test start --port 0` → prints `{"port": N, "pid": P}`
   - `picasu-test given --scenario <file>` → executes setup, prints vars as JSON
   - `picasu-test when --scenario <file> --step N` → executes single step
   - `picasu-test stop --pid P` → SIGTERM

9. **Refactor Playwright interpreter**:
   - Replace `backendLauncher.ts` with `picasu-test start/stop`
   - Replace `executeGiven.ts` with `picasu-test given`
   - Keep `interpreter.ts` for browser-only steps (`navigate`, `click`, `fill`, UI assertions)

10. **Remove duplicated code**:
    - Delete `backendLauncher.ts`, `executeGiven.ts`
    - Simplify `interpreter.ts` to browser-only concerns

### Phase 5: Cleanup

11. Remove old `backend_api.rs` scenario runner (replaced by harness)
12. Remove `build.rs` codegen for scenarios (harness parses YAML directly)
13. Update `justfile` recipes
14. Update `docs/test-strategy.md`

## Repo Design and Testing Philosophy

This repo follows a clear separation of concerns:

- **Backend tests** observe only HTTP responses and filesystem layout (no internal DB inspection)
- **Playwright tests** observe UI rendering and browser interaction
- **Unit tests** cover pure logic that the compiler can't catch

The proposed change aligns with these principles:

- The Rust harness maintains the HTTP/filesystem observation boundary
- Playwright keeps its browser-interaction focus
- The unified DSL reduces the surface area for drift between backend and E2E tests

The child-process model is already proven by `backendLauncher.ts`. Moving it to Rust makes it available to both test systems and enables perf testing.

## Pros / Cons

### Pros

1. **Single DSL** — `given`/`when`/`then` written once, consumed by both Rust and Playwright runners. Eliminates drift between the two implementations.
2. **Watcher coverage** — First time the filesystem→watcher→index pipeline is tested end-to-end. Previously only simulated via API.
3. **Background task coverage** — Dedup, flush, and expire tasks exercised through real process execution, not just API triggers.
4. **Performance testing** — Rust-native latency measurement with zero framework overhead. Enables p50/p95/p99 tracking.
5. **Faster iteration** — Rust scenarios run in ~1s (child process) vs Playwright's ~5s+ (browser overhead). Backend-only tests don't need a browser.
6. **Parallel scenarios** — Each scenario gets its own backend process (isolated port, data dir). No more `TEST_SERIAL_GUARD` bottleneck.
7. **Reduced maintenance** — One YAML interpreter instead of two. One process launcher instead of two.
8. **Consistent with existing architecture** — `backendLauncher.ts` already spawns the real binary. This formalizes the pattern in Rust.

### Cons

1. **Significant refactoring** — 67 YAML scenarios need syntax adaptation. `backend_api.rs` scenario runner (~400 lines) replaced. `backendLauncher.ts` + `executeGiven.ts` (~560 lines) replaced.
2. **New dependency** — `picasu-test` crate becomes a dev-dependency. Adds build time for `cargo test`.
3. **Coordination complexity** — Playwright and Rust runners must agree on the YAML schema. Schema changes require updating both consumers.
4. **Process management overhead** — Each Rust scenario spawns a child process (~100ms startup). 67 scenarios × 100ms = ~7s added to test suite.
5. **Debugging harder** — Failures in child-process tests are harder to diagnose than in-process tests (no access to internal state).
6. **Transition risk** — Running old and new runners in parallel during migration doubles CI time temporarily.
7. **CLI bridge for Playwright** — Playwright calls `picasu-test` as a subprocess, adding IPC overhead and a new failure mode.
8. **Not all scenarios benefit** — Pure HTTP contract tests (e.g., `upload_basic`) don't need a real backend process. The in-process Client is faster for these.

## Progress

_Notes appended below, newest first._
