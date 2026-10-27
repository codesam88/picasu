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

3. Test behavior and implementation health separately. Passing workflow tests
   does not prove that public contracts are documented, architectural
   boundaries are preserved, or complexity and duplication have not grown.
   Those properties need their own executable checks and explicit budgets.

4. All tools run locally for quick turnaround. CI orchestrates slower tests
   and higher-level processes (PR, release). Automate testing across all
   supported build configurations.

---

## Overall pipeline

A layered pipeline that catches defects at the earliest possible stage:

| Layer            | What it catches                                     | How                                                 |
| ---------------- | --------------------------------------------------- | --------------------------------------------------- |
| Static (compile) | Category errors, unsafe code, style violations      | TypeScript, clippy, ESLint, `cargo fmt`/`prettier`  |
| Unit             | Pure-function logic errors                          | `#[cfg(test)]` blocks via `cargo test`              |
| Integration      | Multi-component interaction bugs                    | `src/tests/` against real redb in a tempdir         |
| E2E — API        | HTTP contract violations, regressions               | YAML scenarios → Rocket `local::Client`             |
| E2E — UI         | Full-stack user-flow regressions                    | YAML scenarios → Playwright                         |
| Contract         | Public API drift and compatibility errors           | OpenAPI generation, parity, lint, diff, smoke tests |
| Property/state   | Invariant and sequence errors                       | Property-based and model-based tests                |
| Mutation         | Tests that pass despite meaningful behavior changes | Targeted mutation testing                           |
| Architecture     | Dependency violations, churn, and code smells       | Boundary rules, quality budgets, artifact scans     |
| Precommit        | Format/lint plus branch-configured checks           | `just precommit`                                    |
| CI               | Current suite, audit, and release-build checks      | GitHub Actions                                      |

Each layer filters defects the previous one cannot catch. Static analysis
cannot verify multi-step index → dedup → flush → album update; integration
tests can. Integration tests cannot verify HTTP response shape after a
config change; API E2E can. API E2E cannot verify the login page renders;
UI E2E can.

The Contract, Property/state, Mutation, and Architecture rows describe the
target quality pipeline. The current repository has partial checks in these
areas; the full gates are tracked as backlog and
must not be described as active until their commands are implemented and run.

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
**Run:** `cargo test`

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
**Run:** `just frontend-playwright` (requires `npm run build` first — the backend
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
handlers. `#[utoipauto(paths = …)]` in `backend/src/openapi.rs` collects the
annotations; `build.rs` writes the YAML scenario tests and inspects no route.
`just openapi-check` runs four phases: `openapi-sanity` over the annotations and
the route attributes beside them, a diff of the committed artifact against a
fresh generation, Spectral over the document, and `picasu --check-openapi`,
which compares the routes a shipped-shaped build mounts against that artifact.

The route-set half of the contract is therefore enforced: a mounted public route
missing from the spec, and a spec operation no build mounts, both fail. The
annotation half is enforced as well — the response statuses a handler can answer,
the parameter and body declarations against the route, the tag vocabulary and
the doc-comment provenance of summaries are checked by `openapi-sanity`. Still
review-time: parameter descriptions (21 of the 22 published parameters have none),
the security schemes (none are registered), and the semantics of string-valued
identifiers. The first two are tracked as backlog.

See `docs/openapi-generator.md` for the full design, and
`docs/openapi-reference.md` for the rendered reference.

The scenario `call:` verb currently validates method+path pairs against
`openapi.json` (unidirectional: scenario → spec). A bidirectional trace would
show which endpoints lack scenario coverage and which scenarios cover which
endpoints; this is one part of the planned contract gate.

### Structural and behavioral quality checks

The current pipeline is strongest at example-based behavior. It does not yet
measure whether tests detect altered behavior or whether a change increases
implementation risk. The following extensions are tracked as backlog:

- Property-based and stateful/model-based tests for indexing idempotence,
  watcher reconciliation, album membership, deletion, duplicate handling, and
  identity invariants.
- Mutation testing for high-risk domains and changed modules.
- Architecture boundary checks and forbidden legacy-artifact scans.
- Changed-code budgets for complexity, file size, duplication, dependency
  fan-in/fan-out, nesting, ignored tests, unchecked errors, and broad types.
- Change-impact reports that make broad churn visible even when feature tests
  pass.

These checks complement API and UI scenarios. They test properties of the
implementation and the test suite itself rather than adding more examples of
the same workflows.

### Priority: auth first, then the same shape elsewhere

Assurance effort prioritizes user authentication: it is the first
domain held to the document-exactly/derive/enforce standard (flows in
route matrices, gate predicates and status maps, enforced by tests
and generated-artifact parity). Backend API assurance holds second
priority, configuration and other functional domains third. Each
domain reuses the same enforcement shape — analysis derived from
definitions, tests derived from contracts, drift killed by parity
gates — rather than inventing its own.

---

## API-level assurance (Schemathesis)

Black-box assurance derived from `backend/openapi.json` against a seeded
instance. Sits next to the backend API E2E scenarios and the frontend E2E
suite: those pin hand-written flows; this one generates inputs and sequences
from the contract.

- **Oracle scope (5 checks):** `not_a_server_error`, `status_code_conformance`,
  `content_type_conformance`, `response_schema_conformance`,
  `response_headers_conformance` — schema conformance plus no 5xx. Acceptance
  checks stay off until the spec claims them.
- **Runners (advisory)** — none of these gate PRs; flipping any to gating
  requires the contract to be reconciled and the baseline to be clean:
  - `just dylint-check` — static auth invariants (advisory; zero-finding
    discipline is the gating precondition).
  - `just fuzz-check` — tier 1 single-operation sweep against a throwaway
    seeded instance, compared to `utils/schemathesis/baseline.yaml`.
  - `just fuzz` — tier 2 stateful campaign against a running seeded instance.
  - `just schemathesis-run` — raw escape hatch over a running instance.
- **Quarterly re-pin cadence** applies to Schemathesis (pinned in the
  justfile as `schemathesis_version`) and the other pinned tools.
- **Tool knowledge layer:** `docs/briefs/` (e.g. `docs/briefs/schemathesis.md`)
  holds the research digests; the harness lives in `utils/schemathesis/`.

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

- **`PICASU_WEAK_PWHASH`**: test-only argon2id cost override. Set to
  exactly `1` to run argon2id at trivial cost (m=8192 KiB, t=1, p=1)
  for both user and share password hashing; unset or any other value
  keeps production params (m=19456 KiB, t=2, p=1). Wired into
  `just backend-test` and the e2e backend launcher. The override is
  env-only and read per hash call, so tests can flip it within one
  process; never set in production.
  link the lib without `cfg(test)`.
- **clippy `unwrap_used`**: enforced under `-D warnings` over
  `--all-targets`, so test code is gated the same as lib and bins.
- **pinned toolchain**: `rust-toolchain.toml` pins the exact stable release
  (with `clippy` and `rustfmt` components) so dev machines and CI can never
  drift apart — a new upstream lint cannot break the gate by surprise.
  Dependabot's `rust-toolchain` updates propose the bump as a reviewable PR;
  CI stays green on the pin until it merges.
- **`cargo deny check`**: license and advisory audit, runs via `just audit`.
- **`npm audit`**: runs in CI on every PR, not in precommit (too slow, some
  findings intentional via `overrides`).
- **`npx knip`**: periodic dead-code sweep for the frontend; unsuitable for
  precommit.

### Known gaps

Gaps in the strategy itself (product-behavior gaps live in bug tasks,
not here):

- No frontend component mount harness (`@vue/test-utils`/jsdom absent):
  component contracts are pinned structurally from source, with
  behavior-level rewrites outstanding.
- The full Playwright suite is not part of the precommit gate (it runs
  via `just test` / `just frontend-playwright`), so e2e regressions are
  only caught when someone runs it.
- Scenario `covers` declarations are advisory: `tracer.compare` warns
  on missing API/UI coverage but no run fails on it.
- Sequence/flow-level behavior has no conformance layer yet; flows are
  pinned per-case only (planned in `flow-model-conformance.md`).
