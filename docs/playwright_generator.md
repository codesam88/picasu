# Playwright UI Scenario Generator

## Motivation

The frontend is a Vue SPA (single-page application) driven by a backend JSON
API. Unit-testing Vue components in isolation catches rendering bugs, but
cannot verify that real interactions — navigation, form submission, toast
notification, redirect — work correctly against an actual backend. End-to-end
tests in a real browser fill that gap.

Rather than writing Playwright tests by hand (brittle, expensive to maintain),
this project drives them from **YAML scenario files** that declare fixtures,
user actions, and assertions in a small, interaction-oriented DSL. A thin
interpreter translates each scenario into Playwright API calls at runtime.
The same scenario can be replayed without changes if the underlying UI is
rewritten, as long as ARIA roles and accessible names are preserved.

## Pipeline

```
┌──────────────────────┐     ┌─────────────────────┐
│  scenarios/*.yaml    │     │  schema.json         │
│  (fixture + action + │     │  (structural schema) │
│   assertion DSL)     │     │                      │
└──────┬───────────────┘     └──────┬──────────────┘
       │                            │
       ▼                            ▼
┌──────────────────────────────────────────────┐
│           loadScenarios.ts                   │
│                                              │
│  1. Read *.yaml from scenarios/ directory    │
│  2. Parse with js-yaml                       │
│  3. Validate against Zod schema (types.ts)   │
│  4. Return UiScenario[]                      │
└───────────────────┬──────────────────────────┘
                    │
                    ▼
┌──────────────────────────────────────────────┐
│           interpreter.spec.ts                │
│                                              │
│  For each scenario:                          │
│    createGivenContext(scenario.name)          │
│    executeGiven(request, given, ctx, tracer)  │
│                                              │
│    ┌─ if steps: ────────────────────────┐    │
│    │ executeSteps(page, steps, ctx,     │    │
│    │   tracer)                          │    │
│    │   └─ for each step:                │    │
│    │      executeWhen → executeAssert   │    │
│    └────────────────────────────────────┘    │
│    ┌─ else (flat when/assert): ────────┐    │
│    │ executeWhen(page, when, ctx)       │    │
│    │ executeAssert(page, assert, ctx,   │    │
│    │   tracer)                          │    │
│    └────────────────────────────────────┘    │
│                                              │
│    compare(tracer, covers)  → warnings        │
└───────────────────┬──────────────────────────┘
                    │
                    ▼
┌──────────────────────────────────────────────┐
│   Backend (real Rocket instance)             │
│   Frontend (built dist/ served by Rocket)    │
│   Browser (headed or headless Chromium)      │
└──────────────────────────────────────────────┘
```

## Usage

### Prerequisites

The backend must be built, and the frontend must be built to `dist/` — the
e2e test does not use Vite's dev server. Playwright browsers must be
installed:

```sh
cd gallery-backend && cargo build
cd gallery-frontend && npm run build && npx playwright install chromium
```

### Run

```sh
cd gallery-frontend
npx playwright test --grep "UI scenarios"
```

This starts a Rocket backend on an ephemeral port (30000–59999) with an
isolated data directory under `.testruns/playwright-RUN_ID/`, runs every
YAML scenario in `tests/playwright/scenarios/`, then shuts the backend
down.

### Output layout

Each run creates a unique directory:

```
.testruns/
  playwright-<run-id>/
    data/               # backend data directory (redb, thumbnails)
    config/             # backend config directory (config.json)
    images/             # backend image store (seeded fixture files)
    coverage/           # per-scenario coverage reports
    report.json         # Playwright JSON report
    html-report/        # Playwright HTML report (with trace viewer)
```

The main process computes the full `E2E_DIR` path once and stores it in
`process.env.TESTRUN_DIR`. Workers inherit it via `child_process.fork`
and read it back — they have no knowledge of the `.testruns/playwright-`
naming convention.

### Configuration

| Variable        | Effect                          |
| --------------- | ------------------------------- |
| `UROCISSA_PORT` | Backend port (random if unset)  |
| `TESTRUN_DIR`   | Forces a specific run directory |

## Components

### Scenario DSL (`scenarios/*.yaml`)

Each file declares one scenario with optional `covers:`, required
`given:`, `when:`, and `assert:` blocks. Full verb reference in
`docs/scenario-dsl.md`.

```yaml
name: Login via button click succeeds
covers:
  api:
    - POST /post/authenticate
    - PUT /put/config/password
  ui:
    - route:/home
given:
  - empty: true
  - config:
      password: e2e_test_pwd
when:
  - navigate: /login
  - fill: textbox/Password
    value: e2e_test_pwd
  - click: button/Login
assert:
  - ui.route: /home
```

#### `covers:` (optional)

Declares what the scenario intends to exercise:

- `covers.api` — HTTP method + path pairs the given phase calls (seed
  operations). Each entry is a string like `"POST /post/authenticate"`.
- `covers.ui` — assertion targets the assert phase uses. Format depends on
  the verb: for role/label assertions the raw target (e.g. `main/`), for
  others a prefixed string (`route:/home`, `toast:error:unauthorized`,
  `snapshot:login-page`).

After the scenario runs, the tracer compares expected vs actual and logs
advisory warnings for any declared entry that was never exercised.
Warnings do not fail the test.

### Type definitions (`types.ts`)

Zod schemas for the full DSL: given items, when verbs, assert assertions,
covers block, and the top-level `UiScenario` wrapper. All YAML files are
validated against these schemas at load time (hard error on mismatch).

### Schema validation (`schema.json`)

JSON Schema copy of the Zod rules, used by editors for inline validation
of `.yaml` files. Generated manually — keep in sync with `types.ts`.

### Scenario loader (`loadScenarios.ts`)

Reads `scenarios/*.yaml`, parses each with `js-yaml`, validates with
the Zod `UiScenario` schema, returns the array. Runs once at module load
time in the Playwright worker process.

### Given executor (`executeGiven.ts`)

Maps `given:` entries to real backend calls:

| YAML form         | Action                                            |
| ----------------- | ------------------------------------------------- |
| `empty: true`     | No-op                                             |
| `dir_album: ...`  | Creates directory on disk under `IMAGE_HOME`      |
| `photo: ...`      | Writes a minimal JPEG to `IMAGE_HOME`             |
| `remove: ...`     | Deletes a file from `IMAGE_HOME`                  |
| `config: { ... }` | Sets `readOnlyMode` and/or `password` via PUT API |

Variables bound by `id_as` are stored in `GivenContext.vars` and
interpolated as `${name}` in when-step strings.

The function accepts an optional `CoverageTracer` — when provided, it
wraps the `APIRequestContext` in a Proxy that records every HTTP call
(method + path) made during seeding.

### When interpreter (`interpreter.ts`, `executeWhen`)

Maps `when:` verbs to Playwright page actions:

| YAML verb     | Playwright call                                              |
| ------------- | ------------------------------------------------------------ |
| `navigate`    | `page.goto(path)`                                            |
| `click`       | `page.getByRole(role, { name }).click()`                     |
| `click.text`  | `page.locator('.parent').filter({ hasText }).first().click()` |
| `click.icon`  | `page.locator('button:has(.{class})')` (retries up to 5× with 2s visibility check) |
| `click.first` | `page.locator('.desktop-small-image').first().click()` (retries up to 3×, waits for URL `/view/`) |
| `fill`        | `page.getByRole(...).fill(value)`                             |
| `select`      | `page.getByRole(...).selectOption()`                         |
| `submit`   | `page.keyboard.press('Enter')`           |

Element references are ARIA role/accessible name pairs (`role/name`),
never CSS selectors.

### Assert interpreter (`interpreter.ts`, `executeAssert`)

Maps `assert:` verbs to Playwright `expect` assertions:

| YAML verb               | Assertion                                  |
| ----------------------- | ------------------------------------------ |
| `ui.visible`            | `toBeVisible()`                            |
| `ui.hidden`             | `not.toBeVisible()`                        |
| `ui.text + contains`    | `toContainText(value)`                     |
| `ui.text_visible`       | `getByText(text).first().toBeVisible()`    |
| `ui.chip_visible`       | `[id="album-chip"]` or `[id="filename-chip"]` chip visible with text |
| `ui.sidebar_visible`    | `#abstractData-col` contains text          |
| `ui.count + equals`     | `locator(selector).toHaveCount(n)`         |
| `ui.route`              | `toHaveURL(regex)`                         |
| `ui.modal`              | `dialog.toBeVisible() / not.toBeVisible()` |
| `ui.toast`              | Snackbar visible with matching text        |
| `ui.aria_snapshot`      | `toMatchAriaSnapshot({ name })`            |
| `api.response`          | Fetch URL and assert status code(s)        |

The function accepts an optional `CoverageTracer` — when provided, it
records the verb and target of every assertion.

### Tracer (`tracer.ts`)

Passive instrument that records what actually happened during a scenario:

- **API calls:** method + path for every `request.fetch/post/put/get/delete`
  call made during seeding (intercepted via a Proxy on `APIRequestContext`).
- **UI assertions:** verb + target for every `executeAssert` assertion.

After the scenario, `tracer.compare(covers)` returns a list of
`CoverageWarning` entries for expected API/UI items that were never
observed. Warnings are logged to stderr and written to
`.testruns/playwright-RUN_ID/coverage/<scenario-slug>.json`.

### Test harness (`interpreter.spec.ts`)

The Playwright test file that ties everything together. For each scenario
loaded by `loadScenarios`:

1. Reset the auth token cache (`resetAuthToken()`).
2. Create a `CoverageTracer`.
3. Create a `GivenContext`.
4. Execute given steps (seed state via `executeGiven`).
5. Execute scenario steps:
   - If the scenario uses `steps:`, iterate each when/assert pair
     via `executeSteps`.
   - Otherwise, run `executeWhen` then `executeAssert` for the flat
     when/assert block.
6. Compare tracer records against `covers:` declarations.
7. Write per-scenario coverage report.

Parallel workers each start their own backend instance on a unique port.

### Paths (`paths.ts`)

Generates run-scoped paths at module load time; the main process stores
them in `process.env` so that worker processes (which inherit the
environment) read the same values without knowing the naming convention:

- **`E2E_DIR`** — full path to `.testruns/playwright-<runId>/`, stored in
  `process.env.TESTRUN_DIR`. Workers read this one variable instead
  of reconstructing the path from components.
- **`port`** — random port in 30000–59999, stored in
  `process.env.TESTRUN_PORT`. Workers inherit it, so all
  processes connect to the same backend.
- **`BACKEND_URL`** — `http://localhost:<port>`.
- **`ADMIN_PASSWORD`** — `e2e_test_pwd`, used consistently across all
  login scenarios.

### Backend config (`builder.rs`)

The backend checks `UROCISSA_PORT` and `UROCISSA_ADDRESS` environment
variables and applies them over `config.json` values before building the
Rocket configuration. This is how each e2e worker forces a unique port
without modifying config files.

## Isolation model

| Concern                 | Mechanism                                                |
| ----------------------- | -------------------------------------------------------- |
| Data directory          | Unique `E2E_DIR` per run                                 |
| Backend port            | Unique `UROCISSA_PORT` (random 30000–59999) per run      |
| Auth token              | Reset before each scenario (`resetAuthToken()`)          |
| Parallel workers        | Each worker starts its own backend (fullyParallel: true) |
| Run directory discovery | `TESTRUN_DIR` inherited via `fork()`                     |
