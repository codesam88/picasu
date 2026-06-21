# Scenario DSL

Semi-formal spec and authoring guide for spec-driven E2E testing.

Two scenario types share the `given:` vocabulary but have disjoint
`when:`/`assert:` verb sets:

- **API scenarios** (`gallery-backend/tests/scenarios/*.yaml`) — compiled
  at build time into Rocket `local::Client` tests via `build.rs`. Test
  backend HTTP endpoints directly with no browser.
- **UI scenarios** (`gallery-frontend/tests/playwright/scenarios/*.yaml`)
  — loaded at runtime by the Playwright test runner via `loadScenarios.ts`.
  Drive a real browser against a running backend + built frontend.

## Common structure

Every scenario file is a YAML document with one required top-level key
and several optional:

```yaml
name: Human-readable name for the scenario
covers: # optional — see § Coverage intent
  api:
    - POST /post/authenticate
  ui:
    - textbox/Password
given: # optional — fixture definitions
  - ...
# Either flat when/assert (single-step):
when: ...
assert: ...
# Or multi-step:
steps:
  - when: ...
    assert: ...
```

## `given:` vocabulary (shared)

Each entry in `given:` seeds state. Some forms may bind a result to
`id_as` for later reference in `when:` bodies and `assert:` assertions.
Variables are interpolated as `${variable_name}` in string values across
all verb blocks.

| Form                | Description                                     | Available in |
| ------------------- | ----------------------------------------------- | ------------ |
| `empty: true`       | No-op; signals intent to start from clean state | API, UI      |
| `dir_album: <path>` | Create a directory album on disk                | API, UI      |
| `photo: <path>`     | Write a minimal JPEG to the image store         | API, UI      |
| `remove: <path>`    | Remove a file from the image store              | API, UI      |
| `config: { ... }`   | Set backend config via HTTP API                 | UI only      |

Optional modifier fields:

| Field                  | Applies to       | Description                               |
| ---------------------- | ---------------- | ----------------------------------------- |
| `id_as: <name>`        | dir_album, photo | Binds result to `${name}`                 |
| `tags: [<tag>, ...]`   | photo            | Sets photo tags                           |
| `exif_date: <string>`  | photo            | Sets `DateTimeOriginal`                   |
| `color: [<r>,<g>,<b>]` | photo            | Sets pixel colour (decoded fixtures only) |

### `config:` (UI only)

Sets backend runtime configuration via the HTTP API before the browser
interacts with the page. Accepted fields:

```yaml
- config:
    read_only_mode: true # PUT /put/config { readOnlyMode: true }
    password: hunter2 # PUT /put/config/password
```

Password is set before `readOnlyMode` so the scenario can configure
authentication before locking the API behind it.

## API scenarios (`gallery-backend/tests/scenarios/`)

Compiled at build time by `build.rs` into one `#[test]` per YAML file.
The runtime interpreter lives in `src/tests/backend_api.rs`. Run with
`cargo nextest run`.

### `when:` — single API call

```yaml
when:
  call: <method> <path> # e.g. "PUT /put/assign_album"
  body: <json-value> # request body; `${var}` interpolation
  auth: <true|false> # default true; attaches admin auth cookie
```

`call` is validated against `openapi.json` for operation existence at
build time.

### `assert:` — assertions (one or more)

| Form                            | Assertion                     |
| ------------------------------- | ----------------------------- |
| `response.status: <code>`       | HTTP status code              |
| `response.<json-path>: <value>` | JSON body field matches value |
| `response.<json-path> exists`   | JSON body field is present    |
| `response.<json-path> absent`   | JSON body field is absent     |
| `file_exists: <path>`           | File exists on disk           |
| `file_absent: <path>`           | File does not exist on disk   |

`<json-path>` is a dot-separated path into the response JSON, e.g.
`prefetch.locateTo` or `prefetch.timestamp`.

### Multi-step chains

Use multiple scenarios or a multi-step `when:` block:

```yaml
when:
  - call: PUT /put/assign_album
    body: { hash: "${photo}", album_id: "${album}" }
    capture: response
  - call: GET /get/get-albums
    auth: true
```

### Escape-hatch policy (API)

No raw-Rust escape hatch for assertions. A missing assertion form is
resolved by adding a reusable verb to the vocabulary, not by inlining
code.

## UI scenarios (`gallery-frontend/tests/playwright/scenarios/`)

Loaded at runtime by `loadScenarios.ts`, validated against Zod schemas
(`types.ts`), and executed by `interpreter.spec.ts`. No code generation
step — the YAML is interpreted directly by Playwright.

### Scenario structure

A UI scenario either uses flat `when`/`assert` (a single interaction
followed by assertions) or `steps` (a list of interleaved
interaction–assertion pairs):

```yaml
# Flat form — single when, then assert
name: Simple page load
when:
  - navigate: /
assert:
  - ui.visible: main/

# Stepped form — sequential when/assert pairs
name: Multi-step flow
steps:
  - when:
      - navigate: /login
    assert:
      - ui.visible: textbox/Password
  - when:
      - fill: textbox/Password
        value: my_password
      - click: button/Login
    assert:
      - ui.route: /home
```

### `when:` — user interactions (ordered list)

Elements are referenced by **ARIA role** and **accessible name** (never
CSS selectors).

| Verb                                      | Description                                    |
| ----------------------------------------- | ---------------------------------------------- |
| `navigate: <route>`                       | Go to a URL pattern (e.g. `/`, `/albums/<id>`) |
| `click: <role>/<label>`                   | Click element by ARIA role + accessible name   |
| `fill: <role>/<label>, value: <value>`    | Type into an input                             |
| `select: <role>/<label>, option: <label>` | Choose from listbox/select                     |
| `submit:`                                 | Submit the current form                        |

New interactions → extend the vocabulary with a new verb. No raw-TypeScript
escape hatch.

### `assert:` — UI assertions (one or more)

| Form                                        | Assertion                                                            |
| ------------------------------------------- | -------------------------------------------------------------------- |
| `ui.visible: <role>/<label>`                | Element is visible                                                   |
| `ui.hidden: <role>/<label>`                 | Element is hidden/absent                                             |
| `ui.text: <role>/<label>, contains: <text>` | Element text includes string                                         |
| `ui.toast: type: <type>, contains: <text>`  | Toast of given type (`error`/`success`/`warning`) with matching text |
| `ui.modal: open                             | closed` — Modal dialog state                                         |
| `ui.route: <pattern>`                       | Current URL matches pattern                                          |
| `ui.aria_snapshot: <name>`                  | Compare ARIA role/name/state tree against committed snapshot         |

New assertions → extend the vocabulary with a new verb. No raw-TypeScript
escape hatch.

### `steps:` — multi-step scenarios

Use `steps` when a scenario needs to checkpoint state mid-flow (e.g.
verify a toast appeared before the page navigates). Each step is a
`when`/`assert` pair executed sequentially. The `given` block runs once
before all steps.

```yaml
steps:
  - when:
      - navigate: /login
    assert:
      - ui.visible: textbox/Password
  - when:
      - fill: textbox/Password
        value: wrong
      - click: button/Login
    assert:
      - ui.toast:
          type: error
          contains: unauthorized
  - when:
      - fill: textbox/Password
        value: correct
      - click: button/Login
    assert:
      - ui.route: /home
```

### `covers:` (optional)

Declares what the scenario intends to exercise. After the scenario runs,
the tracer compares expected vs actual and logs advisory warnings for any
unexercised declaration. Warnings do not fail the test.

```yaml
covers:
  api:
    - POST /post/authenticate
    - PUT /put/config/password
  ui:
    - textbox/Password
    - route:/home
```

- `covers.api` — HTTP method + path pairs (e.g. `"PUT /put/config"`).
  Matched against API calls recorded during the `given:` phase via the
  `CoverageTracer`.
- `covers.ui` — assertion target strings. For role/label assertions this
  is the raw target (e.g. `main/`); for others a prefixed form
  (`route:/albums`, `toast:error:unauthorized`, `snapshot:login-page`).

Full coverage tracing design in `docs/playwright_generator.md`.

### Escape-hatch policy (UI)

Same as API: no raw-TypeScript. A missing interaction or assertion verb
is a gap in the DSL, not a reason to inline code. Extend the vocabulary
in this document and the interpreter in `interpreter.ts`.

## Schema validation

The DSL has separate JSON Schemas at
`gallery-backend/tests/schema.json` (API) and
`gallery-frontend/tests/playwright/schema.json` (UI). All scenario files
are validated at load/compile time — a schema mismatch is a hard error.

API scenarios are validated at build time by `build.rs`. UI scenarios are
validated at runtime by `loadScenarios.ts` via the Zod `UiScenario`
schema in `types.ts`.

## Idempotency and isolation

- Each scenario creates its own fixtures under namespaced paths
  (sanitized scenario name used as a directory prefix within `IMAGE_HOME`).
- In API scenarios, all assertions are made through the HTTP API only
  — no direct redb access.
- In UI scenarios, state is seeded before the browser navigates to the
  page under test. Auth tokens are reset per scenario.
