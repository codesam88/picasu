# Test Strategy

## Principles

1. Test logic the compiler cannot verify. Don't test what the type system,
   the framework, or the serialization library already guarantee.

   - **Test** pure functions with non-trivial branches, schema contracts at
     boundaries where silent corruption is possible, integration paths where
     components interact in ways types don't constrain.
   - **Don't test** Rocket routing, redb read/write primitives, bitcode
     round-trips on unchanged structs, or Vue rendering internals.

2. Build self-checking test generators as a common source of truth for
   declaring behavior and measuring success. This catches code drift and
   vibe-coded artifacts that hand-written tests miss, and enables scaling
   without proportional maintenance cost.

3. All tools run locally for quick turnaround. CI orchestrates slower tests
   and higher-level processes (PR, release). Automate testing across all
   supported build configurations.

---

## Overall pipeline

A layered pipeline that catches defects at the earliest possible stage:

| Layer            | What it catches                                       | How                                                |
| ---------------- | ----------------------------------------------------- | -------------------------------------------------- |
| Static (compile) | Category errors, unsafe code, style violations        | TypeScript, clippy, ESLint, `cargo fmt`/`prettier` |
| Unit             | Pure-function logic errors                            | `#[cfg(test)]` blocks via `cargo nextest`          |
| Integration      | Multi-component interaction bugs                      | `src/tests/` against real redb in a tempdir        |
| E2E — API        | HTTP contract violations, regressions                 | YAML scenarios → Rocket `local::Client`            |
| E2E — UI         | Full-stack user-flow regressions                      | YAML scenarios → Playwright                        |
| Precommit        | Format, lint, unit+integration+API-E2E, openapi drift | `just precommit`                                   |
| CI               | All of the above + audit + release build              | GitHub Actions                                     |

Each layer filters defects the previous one cannot catch. Static analysis
cannot verify multi-step index → dedup → flush → album update; integration
tests can. Integration tests cannot verify HTTP response shape after a
config change; API E2E can. API E2E cannot verify the login page renders;
UI E2E can.

Two build configurations: **developer** (debug, no `embed-frontend`, fast
iteration) and **production** (release, `embed-frontend`, self-contained
binary). `just precommit` runs the developer configuration. CI runs both
to catch configuration-specific divergence before a release.

---

## Custom components

### API scenario generator

**Location:** `backend/tests/scenarios/*.yaml`
**Runner:** `build.rs` generates one `#[test]` per YAML file, delegating to
the runtime interpreter in `src/tests/backend_api.rs`.
**Run:** `cargo nextest run`

Each YAML file is a given/when/assert spec for a real Rocket instance with
ephemeral `IMAGE_HOME` and `DATA_HOME`. Internal redb state is opaque —
tests observe only HTTP responses and filesystem layout. This boundary
ensures the E2E suite survives internal storage changes as long as the
exposed behaviour is preserved.

**Given** materialises state via fixtures (`src/tests/fixtures/`): real
JPEGs with embedded XMP/EXIF metadata, directory albums, config overrides.
Fixtures are interface adapters — they translate abstract YAML declarations
into HTTP calls and filesystem operations. When the backend's public API
changes, only the fixtures change; the interpreter and the YAML scenarios
remain untouched.

**When** dispatches HTTP calls through Rocket's `local::Client`.
`capture` and `calc` blocks chain multi-call scenarios where one response
feeds the next request.

**Assert** validates against response status, JSON body (dot-path navigation,
`array_min_counts`, `array_where`), filesystem presence/absence, and image
serving.

**Self-validating:** Selftest scenarios (`tests/scenarios/selftest/`)
deliberately have wrong assertions and are expected to panic. A selftest
that does not panic is a test failure — the assertion machinery itself is
under test.

### UI scenario generator

**Location:** `frontend/tests/playwright/scenarios/*.yaml`
**Runner:** Zod-validated loader (`loadScenarios.ts`) drives
`interpreter.spec.ts`.
**Run:** `just frontend-e2e` (requires `npm run build` first — the backend
serves `dist/` directly; Vite's dev server is not used).

Same given/when/assert structure as the API generator, and the **given** step
seeds state through the same backend HTTP API (`executeGiven.ts`).

**When** maps YAML verbs to Playwright page actions: `navigate` →
`page.goto()`, `click` → `getByRole().click()`, etc.

**Assert** maps to Playwright `expect` assertions: `ui.visible`,
`ui.hidden`, `ui.text`+`contains`, `ui.route`.

The Playwright JSON reporter emits structured per-scenario results
(pass/fail, duration, error details, screenshot paths) designed for
AI-driven course-correction during development
(`jq '.suites[0].specs[] | {title, ok, errMsg}'
playwright-report/results.json`).

### OpenAPI spec generator and coverage tracing

Derives an OpenAPI 3.1 spec from `#[utoipa::path]` annotations on route
handlers. `build.rs` checks every handler registered in the `routes![]` macro
for an annotation and prints `cargo:warning=` for any missing during every
build. `just openapi-docs-check` (in `just precommit` on `main`) regenerates
and diffs the committed spec files — unannotated new routes or stale docs
fail the gate.

See `docs/openapi-generator.md` for the full design, and
`docs/openapi-reference.md` for the rendered reference.

**Future: scenario-to-endpoint tracing.** The scenario `call:` verb
currently validates method+path pairs against `openapi.json`
(unidirectional: scenario → spec). A bidirectional trace would show which
endpoints lack scenario coverage and which scenarios cover which endpoints.

---

## Other notes

### What is not tested and why

| Area                                              | Reason                                                                                   |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Rocket route definitions                          | Framework handles dispatch; type-checked at compile time                                 |
| redb read/write primitives                        | Covered by redb's own test suite                                                         |
| bitcode round-trips on unchanged structs          | Covered by bitcode's own test suite; only the _version dispatch logic_ needs testing     |
| Vue component rendering                           | Deferred to E2E; unit-testing rendering adds maintenance cost without proportional value |
| Individual Pinia getters backed by a single field | No logic to test; the type system covers it                                              |

### Tooling caveats

- **clippy `unwrap_used`**: set to `warn` globally (visible in IDEs) but
  excluded from the `-D warnings` precommit flag until the ~140 existing
  call sites are addressed.
- **`cargo deny check`**: license and advisory audit, runs via `just audit`.
- **`npm audit`**: runs in CI on every PR, not in precommit (too slow, some
  findings intentional via `overrides`).
- **`npx knip`**: periodic dead-code sweep for the frontend; unsuitable for
  precommit.

### Known gaps

- **XMP keyword extraction from real file metadata** remains a stub; tags
  can only be injected via fixtures. Porting the `extract_keywords` unit
  tests and `scenario_n` to passing will close this gap and unblock
  format-coverage for video files.
- **Prefetch snapshot expiry** has a race where a snapshot can be deleted
  almost immediately after creation. Tracked in `TODO.md` and pinned by
  intentionally-red tests in the legacy `e2e.rs`, which is being phased out
  as scenarios are ported to the DSL.
