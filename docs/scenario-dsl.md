# Scenario DSL

Semi-formal spec and authoring guide for spec-driven E2E testing.

Two scenario types share the `given:` vocabulary but have disjoint
`when:`/`assert:` verb sets:

- **API scenarios** (`backend/tests/scenarios/*.yaml`) — compiled
  at build time into Rocket `local::Client` tests via `build.rs`. Test
  backend HTTP endpoints directly with no browser.
- **UI scenarios** (`frontend/tests/playwright/scenarios/*.yaml`)
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

| Form                   | Description                                     | Available in |
| ---------------------- | ----------------------------------------------- | ------------ |
| `empty: true`          | No-op; signals intent to start from clean state | API, UI      |
| `dir_album: <path>`    | Create a directory album on disk                | API, UI      |
| `photo: <path>`        | Write a minimal JPEG to the image store         | API, UI      |
| `fixture: <object>`    | Copy a checked-in fixture into the image store  | API          |
| `raw_file: <path>`     | Write UTF-8 text to a path                      | API, UI      |
| `random_media: <path>` | Write the format the run's seed selected        | API          |
| `remove: <path>`       | Remove a file from the image store              | API, UI      |
| `config: { ... }`      | Set backend config via HTTP API                 | UI only      |

Optional modifier fields:

| Field                  | Applies to                              | Description                               |
| ---------------------- | --------------------------------------- | ----------------------------------------- |
| `id_as: <name>`        | dir_album, photo, fixture, random_media | Binds result to `${name}`                 |
| `asset_id_as: <name>`  | photo, fixture, random_media            | Binds the API identity to `${name}`       |
| `tags: [<tag>, ...]`   | photo                                   | Sets photo tags                           |
| `exif_date: <string>`  | photo                                   | Sets `DateTimeOriginal`                   |
| `color: [<r>,<g>,<b>]` | photo                                   | Sets pixel colour (decoded fixtures only) |

### `randomize:` (API only)

Opt a scenario into seeded format selection. It then runs **once per seed** in
the named set, and each pass materialises `random_media` in the format that seed
resolves to. The seed randomises the _input_; the assertions stay the same for
every seed, so they may only assert what holds for every eligible format.

```yaml
randomize:
  seeds: ci # a set name from backend/tests/seeds.json; defaults to `ci`
given:
  - random_media: /e2e_rand_meta/asset # written as asset.<ext>, e.g. asset.tif
    id_as: $photo
    asset_id_as: $asset_id
  - raw_file: /e2e_rand_meta/asset.xmp # sidecar XMP works for every format
when:
  - call: POST /get/prefetch?locate=${asset_id}
    capture:
      token: response.token
      ts: response.prefetch.timestamp
  - call: GET /get/metadata/${asset_id}?timestamp=${ts}
    auth: false
    headers:
      Authorization: "Bearer ${token}"
then:
  - response.status: 200
  - response.json.ext: ${ext}
  - file_exists: e2e_rand_meta/asset.${ext}
```

`random_media` binds three variables for the rest of the scenario: `${format}`
(the selected format's name), `${ext}` (its canonical extension, which is
appended to the path), and `${mime}` (its content type from the capability
manifest, for an `upload` step's `content_type`).

Selection is a filter over `utils/snapfab/capabilities.json`: only formats with
a _verified fixture_ (generated or pinned) and no expected failure beyond `none`
can be picked, so HEIF/HEIC and AVIF can never be selected. The format and its
fixture are recorded per seed in `backend/tests/seeds.json` (`resolvesTo`), and
a test fails if a recorded seed no longer resolves to the format recorded for it.

| Source                       | Effect                                                 |
| ---------------------------- | ------------------------------------------------------ |
| scenario's `randomize.seeds` | The set that runs. Default `ci`.                       |
| `PICASU_RANDOM_SEEDS=<set>`  | Run that set instead (e.g. `nightly`).                 |
| `PICASU_RANDOM_SEEDS=1,5`    | Run exactly those seeds, to replay a reported failure. |

Each pass prints its seed and resolved format, and a failure repeats them in the
panic message:

```
[randomized] <scenario> run 3/6: seed=2 format=tiff ext=tif source=pinned fixture=tiff-48x32-exif
```

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

## API scenarios (`backend/tests/scenarios/`)

Compiled at build time by `build.rs` into one `#[test]` per YAML file.
The runtime interpreter lives in `src/tests/backend_api.rs`. Run with
`cargo test`.

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

| Form                             | Assertion                      |
| -------------------------------- | ------------------------------ |
| `response.status: <code>`        | HTTP status code               |
| `response.status_not: <code>`    | HTTP status is not this code   |
| `response.<json-path>: <value>`  | JSON body field matches value  |
| `response.<json-path> absent`    | JSON body field is absent      |
| `file_exists: <path>`            | File exists on disk            |
| `file_absent: <path>`            | File does not exist on disk    |
| `serve_image_ok: $<hash>`        | The compressed image serves    |
| `thumb_exists: $<hash>`          | The generated thumbnail exists |
| `file.contains: <path>` + `text` | File's text contains `text`    |

`<json-path>` is a dot-separated path into the response JSON, e.g.
`prefetch.locateTo` or `prefetch.timestamp`.

`${var}` interpolation applies to `response.<json-path>` values and to
`file_exists` / `file_absent` paths, so a randomized scenario can assert on the
selected format's extension.

`file_exists`, `file_absent` and `file.contains` take an **IMAGE_HOME-relative**
path with an optional leading `/`. A path that interpolates to an _absolute_ one
(typically `${data_path}/…`) is rejected: joined onto IMAGE_HOME it would
resolve to a location that cannot exist, which makes `file_absent` pass and
`file_exists` meaningless.

`serve_image_ok: $<hash>` fetches
`GET /object/compressed/<hash[0:2]>/<hash>.jpg` — the route the frontend uses —
and requires 200, an `image/jpeg` content type and JPEG bytes. The variable is
the content hash `id_as` binds; token issuance for that route is covered by
`token_hash_compressed_serving.yaml`.

### Multi-step chains

Use multiple scenarios or a multi-step `when:` block:

```yaml
when:
  - call: PUT /put/assign_album
    body: { assetId: "${photo}", albumId: "${album}" }
    then:
      - response.status: 200
      - response.json.outcome: moved
  - call: GET /get/get-albums
```

### Minting a timestamp token (`mint_timestamp_token`)

Timestamp bearer tokens are signed server-side (`exp` included), so a
scenario cannot produce one — let alone an expired one — through
`capture`/`calc` alone. A multi-step `when:` block may include a mint item
instead of a call:

```yaml
when:
  - call: POST /get/prefetch?locate=${asset}
    capture:
      ts: response.prefetch.timestamp
  - mint_timestamp_token:
      as: $expired_token
      timestamp: "${ts}"
      exp_offset: -3600
  - call: GET /get/get-rows?index=0&timestamp=${ts}
    auth: false
    headers:
      Authorization: "Bearer ${expired_token}"
```

`as` binds the signed JWT to `${expired_token}` for later interpolation;
`timestamp` becomes the token's `timestamp` claim (pass the snapshot the
request targets so expiry, not a claim mismatch, is the rejection reason);
`exp_offset` is seconds relative to now (`300`, the app default, when
omitted). A mint produces no HTTP response, so it cannot be the last item
of `when:` — nothing for `then:` to assert against.

A `call:` that is **not** the last one asserts its own inline `then:` block, and
every form in it runs — status, `response.json.*`, `array_min_counts`,
`array_where`, `compare`, `file_*`, `serve_image_ok`. The last call is asserted
by the scenario's top-level `then:`. An inline `then:` is evaluated before the
same call's `capture` and `calc` feed the variables, so it cannot depend on a
value the response it asserts produced.

A `call:` may also bind a discovered identity with `id_as` (content hash) or
`asset_id_as` (API asset id), both resolved by the following `discover_path` —
and each works on its own; `asset_id_as` does not require `id_as`.

### `when:` verbs that change state

Beyond `call:` and `upload:`, a step may act on the filesystem. Each returns a
`GET /get/index/status` probe, and each runs against IMAGE_HOME-relative paths
with an optional leading `/`:

| Verb                                  | Effect                                                           |
| ------------------------------------- | ---------------------------------------------------------------- |
| `wait_index: true`                    | Block until the running album index settles (default: completed) |
| `wait_index: {expect: failed}`        | Block until it settles in the `failed` state                     |
| `write_file: <path>` + `content`      | Write UTF-8 text to a path                                       |
| `truncate_file: {path, bytes}`        | Keep only the first `bytes` bytes of a file                      |
| `duplicate_of: {source, destination}` | Copy a file's bytes                                              |
| `chmod: {path, mode}`                 | Change a path's POSIX permissions                                |

`wait_index: {expect: failed}` is how a scenario observes an album index whose
every matched file failed: that state used to be a harness panic. `truncate_file`
is the way to get there — `write_file` writes UTF-8 text, which matches no media
signature and is therefore skipped before a file is ever matched, so only a
truncated real file can be _matched_ and then fail.

### Escape-hatch policy (API)

No raw-Rust escape hatch for assertions. A missing assertion form is
resolved by adding a reusable verb to the vocabulary, not by inlining
code.

## UI scenarios (`frontend/tests/playwright/scenarios/`)

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

Elements with ARIA labels are referenced by **role** and **accessible name**
(e.g. `button/Login`). Elements without ARIA labels use one of the
text/icon-based verbs below.

| Verb                                      | Description                                                                   |
| ----------------------------------------- | ----------------------------------------------------------------------------- |
| `navigate: <route>`                       | Go to a URL pattern (e.g. `/`, `/albums/<id>`)                                |
| `click: <role>/<label>`                   | Click element by ARIA role + accessible name                                  |
| `click.text: <text>`                      | Click an album card by its chip label (uses `.parent` container)              |
| `click.icon: <icon-class>`                | Click a button by Material Design Icon class (e.g. `mdi-information-outline`) |
| `click.first`                             | Click the first grid image (`.desktop-small-image`) in the active overlay     |
| `fill: <role>/<label>, value: <value>`    | Type into an input                                                            |
| `select: <role>/<label>, option: <label>` | Choose from listbox/select                                                    |
| `submit`                                  | Submit the current form                                                       |
| `wait.ms: <milliseconds>`                 | Pause execution (use sparingly — prefer auto-waiting assertions)              |

New interactions → extend the vocabulary with a new verb. No raw-TypeScript
escape hatch.

### `assert:` — UI assertions (one or more)

| Form                                        | Assertion                                                            |
| ------------------------------------------- | -------------------------------------------------------------------- |
| `ui.visible: <role>/<label>`                | Element is visible                                                   |
| `ui.hidden: <role>/<label>`                 | Element is hidden/absent                                             |
| `ui.text: <role>/<label>, contains: <text>` | Element text includes string                                         |
| `ui.text_visible: <text>`                   | Text is visible anywhere on the page                                 |
| `ui.chip_visible: <text>`                   | Album/filename chip with given text is visible in a grid card        |
| `ui.sidebar_visible: <text>`                | Text is visible inside the metadata sidebar (`#abstractData-col`)    |
| `ui.count: <selector>, equals: <number>`    | Count of elements matching a CSS selector equals the given number    |
| `ui.toast: type: <type>, contains: <text>`  | Toast of given type (`error`/`success`/`warning`) with matching text |
| `ui.modal: open                             | closed` — Modal dialog state                                         |
| `ui.route: <pattern>`                       | Current URL matches pattern                                          |
| `ui.aria_snapshot: <name>`                  | Compare ARIA role/name/state tree against committed snapshot         |
| `api.response: url: <url>, status: <code>`  | Backend API call returns expected status code                        |

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
`backend/tests/schema.json` (API) and
`frontend/tests/playwright/schema.json` (UI). All scenario files
are validated at load/compile time — a schema mismatch is a hard error.

API scenarios are validated at build time by `build.rs`. UI scenarios are
validated at runtime by `loadScenarios.ts` via the Zod `UiScenario`
schema in `types.ts`.

## Idempotency and isolation

- Each Playwright scenario runs its own backend instance with a unique
  `{TEST_DIR}/playwright-{id}/` directory (see `paths.ts`), so fixture
  files on disk are already isolated. No scenario-name directory prefix
  is used — albums are placed at the root of `IMAGE_HOME` and respect
  the `root_album` backend filter.
- In API scenarios, all assertions are made through the HTTP API only
  — no direct redb access.
- In UI scenarios, state is seeded before the browser navigates to the
  page under test. Auth tokens are reset per scenario.
