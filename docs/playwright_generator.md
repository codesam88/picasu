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

> **Important:** The E2E test does **not** use Vite's dev server. The backend
> binary (`picasu`) serves the production build of the frontend directly
> from `frontend/dist/`. You must run `npm run build` before running
> tests. See the pipeline diagram above.

The backend must be built, and the frontend must be built to `dist/` — the
e2e test does not use Vite's dev server. Playwright browsers must be
installed:

```sh
cd backend && cargo build
cd frontend && npm run build && npx playwright install chromium
```

### Run

```sh
cd frontend
npx playwright test --grep "UI scenarios"
```

Each scenario starts its own isolated Rocket backend on an ephemeral port
(30000–59999) with its own data directory under `.testruns/playwright-RUN_ID/`,
runs the YAML scenario against that backend, then shuts it down. Multiple
scenarios run in parallel when Playwright has multiple workers available.

### Output layout

Each scenario gets its own `playwright-<id>/` directory under `TEST_DIR`.
Directories persist after the run for debugging. Artifacts and reports
are consolidated to the shared run directory:

```
.testruns/
  playwright-<run-id>/    (per-scenario, temporary — removed after test)
    data/                 # backend data directory (redb, thumbnails)
    config/               # backend config directory (config.toml)
    images/               # backend image store (seeded fixture files)
    coverage/             # per-scenario coverage reports
  artifacts/              # Playwright run artifacts (shared)
    report.json           # Playwright JSON report
    html-report/          # Playwright HTML report (with trace viewer)
```

### Configuration

| Variable     | Effect                                                                                |
| ------------ | ------------------------------------------------------------------------------------- |
| `TEST_DIR`   | Top-level directory for all test outputs (default: `.testruns/` under repo root)      |
| `WORKER_NUM` | Deterministic worker index (port = `30000 + N*2`, path = `{TEST_DIR}/playwright-{N}`) |
| `CI`         | When set, enables Playwright retries (`retries: 2`) and `forbidOnly`                  |

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

| YAML form                            | Action                                                        |
| ------------------------------------ | ------------------------------------------------------------- |
| `empty: true`                        | No-op                                                         |
| `dir_album: ...`                     | Creates directory on disk under `IMAGE_HOME`                  |
| `photo: ...`                         | Writes a minimal JPEG to `IMAGE_HOME`                         |
| `photo_raw: ...`                     | Writes a file to `IMAGE_HOME` without triggering an index     |
| `source_file: ...`                   | Writes an upload source **outside** `IMAGE_HOME`              |
| `source_file: ... { fixture: <id> }` | Same, from a pinned capability-manifest fixture               |
| `remove: ...`                        | Deletes a file from `IMAGE_HOME`                              |
| `move: { move, to }`                 | Renames a file after seeding, then re-checks the object route |
| `config: { ... }`                    | Sets `readOnlyMode` and/or `password` via PUT API             |

`source_file` items are written outside `IMAGE_HOME` on purpose: the scenario
uploads them through the browser file chooser, so an indexed copy would put the
item in the gallery before the upload step ran. Its `fixture` field names an
entry of `utils/snapfab/capabilities.json` and is the binary-safe alternative to
`format` for the formats `snapfab batch` cannot encode (TIFF, WebP, MP4, MOV);
`pinnedFixtures.ts` verifies the recorded SHA-256 before writing the bytes and
rejects a filename whose extension the pinned format does not declare. The two
fields are mutually exclusive.

Variables bound by `id_as` are stored in `GivenContext.vars` and
interpolated as `${name}` in when-step strings.

The function accepts an optional `CoverageTracer` — when provided, it
wraps the `APIRequestContext` in a Proxy that records every HTTP call
(method + path) made during seeding.

### Pinned fixture placement (`pinnedFixtures.ts`)

`copyPinnedFixture(id, destination)` resolves `id` against the capability
manifest at `utils/snapfab/capabilities.json`, verifies the bytes against the
manifest's recorded SHA-256, checks that the destination's extension is one the
pinned format declares, then writes them. All three checks fail loudly with the
remedy in the message: a scenario that named the wrong fixture would otherwise
assert the wrong format and pass, and a truncated fixture would surface much
later as an unexplained upload or decode failure.

A format whose manifest entry declares `metadataFields.container: [probe]` is
additionally gated on a working ffmpeg and ffprobe, resolved on `PATH`. The
backend reads those formats by shelling out — ffprobe for the dimensions and
metadata map, ffmpeg for the thumbnail — and neither has a pure-Rust fallback,
so without them the upload is rejected and the scenario fails several steps
later with "could not be decoded as an image or video", which reads as a
product bug rather than a missing binary. This mirrors the backend's hard
precondition (`process::video::tests::video_metadata_requires_a_working_ffmpeg_and_ffprobe`);
it fails, it does not skip. Only the video formats are gated, so the image flows
stay runnable without a video toolchain.

This is the UI counterpart of the API generator's `fixture` given step; the
manifest digest is already pinned by `cargo test -p snapfab`, but this harness
reads the manifest file directly rather than through snapfab's loader.

### When interpreter (`interpreter.ts`, `executeWhen`)

Maps `when:` verbs to Playwright page actions:

| YAML verb     | Playwright call                                                                                   |
| ------------- | ------------------------------------------------------------------------------------------------- |
| `navigate`    | `page.goto(path)`                                                                                 |
| `click`       | `page.getByRole(role, { name }).click()`                                                          |
| `click.text`  | `page.locator('.parent').filter({ hasText }).first().click()`                                     |
| `click.icon`  | `page.locator('button:has(.{class})')` (retries up to 5× with 2s visibility check)                |
| `click.first` | `page.locator('.desktop-small-image').first().click()` (retries up to 3×, waits for URL `/view/`) |
| `fill`        | `page.getByRole(...).fill(value)`                                                                 |
| `select`      | `page.getByRole(...).selectOption()`                                                              |
| `submit`      | `page.keyboard.press('Enter')`                                                                    |

Element references are ARIA role/accessible name pairs (`role/name`),
never CSS selectors.

### Assert interpreter (`interpreter.ts`, `executeAssert`)

Maps `assert:` verbs to Playwright `expect` assertions:

| YAML verb            | Assertion                                                            |
| -------------------- | -------------------------------------------------------------------- |
| `ui.visible`         | `toBeVisible()`                                                      |
| `ui.hidden`          | `not.toBeVisible()`                                                  |
| `ui.text + contains` | `toContainText(value)`                                               |
| `ui.text_visible`    | `getByText(text).first().toBeVisible()`                              |
| `ui.chip_visible`    | `[id="album-chip"]` or `[id="filename-chip"]` chip visible with text |
| `ui.sidebar_visible` | `#abstractData-col` contains text                                    |
| `ui.count + equals`  | `locator(selector).toHaveCount(n)`                                   |
| `ui.route`           | `toHaveURL(regex)`                                                   |
| `ui.modal`           | `dialog.toBeVisible() / not.toBeVisible()`                           |
| `ui.toast`           | Snackbar visible with matching text                                  |
| `ui.aria_snapshot`   | `toMatchAriaSnapshot({ name })`                                      |
| `api.response`       | Fetch URL and assert status code(s)                                  |

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

Each scenario starts its own backend instance via `scenarioFixtures.ts`,
which calls `backendLauncher.startBackend()` with a per-scenario path
set. The `page` fixture is overridden to set `baseURL` to the scenario's
backend, so all `page.goto('/login')` calls resolve to the right
instance.

### Paths (`paths.ts`)

- **`TEST_DIR`** — shared top-level directory for all test-run outputs.
  Set via `TEST_DIR` env var; defaults to `.testruns/` under the repo
  root. Used by `playwright.config.ts` for the report and artifact paths.
- **`createPaths()` factory** — called per-scenario to generate fresh,
  isolated paths for each backend instance. Uses `{TEST_DIR}/playwright-<id>/`.
  When `WORKER_NUM` is set, `id` is the worker number for deterministic
  paths and ports (`30000 + N*2`). Otherwise a random 6-char hex string.

| Export / Function | Source                                             |
| ----------------- | -------------------------------------------------- |
| `TEST_DIR`        | `process.env.TEST_DIR` or `{REPO_ROOT}/.testruns/` |
| `createPaths()`   | `{TEST_DIR}/playwright-{WORKER_NUM\|random}/`      |
| `ADMIN_PASSWORD`  | Always `e2e_test_pwd`                              |

## Isolation model

| Concern          | Mechanism                                                  |
| ---------------- | ---------------------------------------------------------- |
| Data directory   | Unique per-scenario `playwright-{id}` under `TEST_DIR`     |
| Backend port     | Unique per-scenario instance (random 30000–59999)          |
| Auth token       | Reset before each scenario (`resetAuthToken()`)            |
| Parallel workers | Each scenario starts its own backend (fullyParallel: true) |
| Path control     | `TEST_DIR` / `WORKER_NUM` env vars                         |
