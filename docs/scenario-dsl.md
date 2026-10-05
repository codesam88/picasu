# Scenario DSL

Semi-formal spec and authoring guide for spec-driven E2E testing.

Two scenario types with overlapping `given:` vocabularies and disjoint
`when:`/assertion verb sets (`then:` for API scenarios, `assert:` for UI):

- **API scenarios** (`backend/tests/scenarios/*.yaml`) — compiled
  at build time into Rocket `local::Client` tests via `build.rs`. Test
  backend HTTP endpoints directly with no browser.
- **UI scenarios** (`frontend/tests/playwright/scenarios/*.yaml`)
  — loaded at runtime by the Playwright test runner via `loadScenarios.ts`.
  Drive a real browser against a running backend + built frontend.

## Common structure

Every scenario file is a YAML document with required and optional
top-level keys. The API type answers to `then:`, the UI type to
`assert:` (or `steps:`):

```yaml
# API scenario:
name: Human-readable name for the scenario
given: # optional — fixture definitions
  - ...
when: ...
then: ... # required key; may be an empty list

# UI scenario:
name: Human-readable name for the scenario
covers: # optional — see § Coverage intent
  api:
    - POST /post/authenticate
  ui:
    - textbox/Password
given: # optional — fixture definitions
  - ...
when: ... # or `steps:` for interleaved when/assert pairs
assert: ...
```

## `given:` vocabulary (shared)

Each entry in `given:` seeds state. Some forms may bind a result to
`id_as` for later reference in `when:` bodies and `then:`/`assert:`
assertions. Variables are interpolated as `${variable_name}` in string values
across all verb blocks.

| Form                     | Description                                                   | Available in |
| ------------------------ | ------------------------------------------------------------- | ------------ |
| `empty: true`            | No-op; signals intent to start from clean state               | API, UI      |
| `dir_album: <path>`      | Create a directory album on disk                              | API, UI      |
| `photo: <path>`          | Generate a JPEG/PNG (`format:`) into the image store          | API, UI      |
| `remove: <path>`         | Remove a file from the image store                            | API, UI      |
| `config: { ... }`        | Set backend config (accepted fields differ per harness)       | API, UI      |
| `raw_file: <path>`       | Write UTF-8 text (`content:`) to a path                       | API          |
| `fixture: { ... }`       | Copy a checked-in manifest fixture into the image store       | API          |
| `random_media: <path>`   | Write the format the run's seed selected (needs `randomize:`) | API          |
| `duplicate_of: { ... }`  | Byte-identical copy of a file placed earlier in `given:`      | API          |
| `truncate_file: { ... }` | Keep only the first `bytes` bytes of a given-phase file       | API          |
| `patch_file: { ... }`    | Hex find/replace in a given-phase file (same-length patterns) | API          |
| `photo_raw: <path>`      | Place a generated image without triggering an index scan      | UI           |
| `source_file: <path>`    | Place a file _outside_ `IMAGE_HOME` for the browser to upload | UI           |
| `move: <path>` + `to`    | Move a file within the image store                            | UI           |

Optional modifier fields:

| Field                  | Applies to                                                | Description                                                                |
| ---------------------- | --------------------------------------------------------- | -------------------------------------------------------------------------- |
| `id_as: <name>`        | dir_album, photo, fixture, random_media, source_file (UI) | Binds a discovered identity to `${name}` (see note below)                  |
| `asset_id_as: <name>`  | photo, fixture, random_media (API)                        | Binds the API asset id to `${name}`                                        |
| `tags: [<tag>, ...]`   | photo                                                     | Sets photo tags                                                            |
| `exif_date: <string>`  | photo                                                     | Sets `DateTimeOriginal`                                                    |
| `color: [<r>,<g>,<b>]` | photo (API)                                               | Sets pixel colour (decoded fixtures only)                                  |
| `width` / `height`     | photo, photo_raw, source_file                             | Generated image dimensions                                                 |
| `format: jpeg\|png`    | photo, photo_raw, source_file                             | Generated image format (default `jpeg`); mutually exclusive with `fixture` |
| `content: <text>`      | raw_file                                                  | File contents (default empty)                                              |

What `id_as` binds depends on the form and the harness: the album id for
`dir_album`; the content hash for API `photo`/`fixture`/`random_media`; the
asset id for UI `photo`; and the placed source path for UI `source_file`.

`truncate_file` and `patch_file` are byte transforms over files an earlier
`given:` item created, applied after generation and before the scan — a
corruption derives from a real file instead of a hand-built blob. `patch_file`
requires the find/replace patterns to be equal length and the pattern to be
present, so a corruption cannot be a silent no-op.

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

Selection is a filter over `utils/snapfab/capabilities.json`: a format is a
candidate only with a _verified fixture_ (generated by snapfab, or a pinned
fixture that resolves) and no expected failure beyond `none`. The format and
its fixture are recorded per seed in `backend/tests/seeds.json`
(`resolvesTo`), and a test fails if a recorded seed no longer resolves to the
format recorded for it.

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

### `config:` (API and UI)

Sets backend runtime configuration before the calls or the browser run.
The API harness accepts `read_only_mode`, `fs_notify_watcher`,
`validate_upload_content` and `use_client_timestamp_info`; the UI harness
additionally accepts `password` and `auth_key`:

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
  raw_body: <string> # literal request body; wins over `body`
  headers: { Authorization: "Bearer ${token}" } # extra request headers
  auth: <true|false> # default true; attaches admin auth cookie
  then: [...] # inline assertions (list); on the last step the top-level `then:` applies
  capture: { var: response.json.<path> } # bind response fields for later steps
  calc: { var: "${other}+1" } # numeric offset expression
  id_as: $name # content hash, resolved after this call by `discover_path:`
  asset_id_as: $name # API asset id, resolved the same way
```

A scenario file's structure and every verb it uses are validated against
`backend/tests/schema.json` (see § Schema validation).

### `then:` — assertions (one or more)

The scenario's top-level `then:` asserts the last `when:` step's response; a
non-final step asserts its own inline `then:` (see below). The same forms are
valid in both places:

| Form                                               | Assertion                                            |
| -------------------------------------------------- | ---------------------------------------------------- |
| `response.status: <code>`                          | HTTP status code                                     |
| `response.status_not: <code>`                      | HTTP status is not this code                         |
| `response.header.<name>: <value>`                  | Response header equals value                         |
| `response.json.<path>: <value>`                    | JSON body field equals value (`${var}` interpolated) |
| `response.json.<path>: not_null`                   | JSON body field is present and not `null`            |
| `response.json.<path>: {contains: <v>}`            | Array field contains value                           |
| `response.json.<path>: {not_contains: <v>}`        | Array field does not contain value                   |
| `response.json.<path>: {all_absolute: true}`       | Every element of the array is an absolute path       |
| `array_min_counts: {<tag>: <n>}`                   | Response tag array holds ≥ `n` entries for `<tag>`   |
| `array_where: {where: …, expect: present\|absent}` | An element matching `where` is (not) present         |
| `compare: {<path>: {<op>: <n>}}`                   | Integer comparison; `<`, `<=`, `>` or `>=`           |
| `file_exists: <path>`                              | File exists on disk                                  |
| `file_absent: <path>`                              | File does not exist on disk                          |
| `file.contains: <path>` + `text`                   | File's text contains `text`                          |
| `file.not_contains: <path>` + `text`               | File's text does not contain `text`                  |
| `thumb_exists: $<hash>` / `thumb_absent: $<hash>`  | The generated thumbnail exists / does not            |
| `serve_image_ok: $<hash>`                          | The compressed image route serves a JPEG             |

Body assertions must use the `response.json.` prefix: `backend_api.rs`
dispatches a body assertion only for a key that starts with it, so a bare
`response.<path>` in a `then:` list would be read and silently dropped —
and the schema must not admit a form that asserts nothing. The path allows
`:` and `-` inside segments (ffprobe-derived video keys are literally named
e.g. `TAG:major_brand`) and `[n]` for array indices.

`not_null` and the `contains`/`not_contains`/`all_absolute` objects are
**form markers**, not values. `not_contains` is the only way to say "the cache
is unchanged" about an edit that _removes_ a value: asserting that the old
value is still there would pass for a cache that stored both the removal and
the addition.

`${var}` interpolation applies to `response.json.` values and to `file_*`
paths, so a randomized scenario can assert on the selected format's extension.

`file_exists`, `file_absent`, `file.contains` and `file.not_contains` take an
**IMAGE_HOME-relative** path with an optional leading `/`. A path that
interpolates to an _absolute_ one (typically `${data_path}/…`) is rejected:
joined onto IMAGE_HOME it would resolve to a location that cannot exist, which
makes `file_absent` pass and `file_exists` meaningless.

`serve_image_ok: $<hash>` fetches
`GET /object/compressed/<hash[0:2]>/<hash>.jpg` — the route the frontend uses —
and requires 200, an `image/jpeg` content type and JPEG bytes. The variable is
the content hash `id_as` binds; token issuance for that route is covered by
`token_hash_compressed_serving.yaml`.

### Multi-step chains

Use multiple scenarios or a multi-step `when:` block:

```yaml
when:
  - call: POST /get/prefetch?locate=${photo}
    then:
      - response.status: 200
      - response.json.prefetch.timestamp: not_null
    capture:
      ts: response.prefetch.timestamp
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

A `call:` (or `upload:`) that is **not** the last one asserts its own inline
`then:` block, and every form in it runs — status, `response.json.*`,
`array_min_counts`, `array_where`, `compare`, `file_*`, `thumb_*`,
`serve_image_ok`. The last step is asserted by the scenario's top-level
`then:`. An inline `then:` is evaluated before the same step's `capture` and
`calc` feed the variables, so it cannot depend on a value the response it
asserts produced; a `then:` that is not a list of assertions is a hard error
rather than something to read and drop.

A `call:` may also bind a discovered identity with `id_as` (content hash) or
`asset_id_as` (API asset id), both resolved by the following `discover_path` —
and each works on its own; `asset_id_as` does not require `id_as`.

### `when:` — upload

An `upload:` step posts a multipart upload through the same interface the UI
uses:

```yaml
when:
  - upload:
      file: /e2e_upload/src/photo.jpg # IMAGE_HOME-relative source placed by `given:`
      filename: photo.jpg # name the server sees; defaults to the source's name
      target_album: "${album}" # optional; album id or path
      content_type: image/jpeg # optional; defaults to image/jpeg
      on_conflict: rename # or `skip`
```

`file` is required; `auth` defaults to true; like `call:`, a non-final
`upload:` may carry an inline `then:`, `capture`, `id_as`/`asset_id_as` +
`discover_path`.

### `when:` verbs that change state

Beyond `call:` and `upload:`, a step may act on the filesystem. The
path-taking verbs accept IMAGE_HOME-relative paths with an optional leading
`/`:

| Verb                                  | Effect                                                         |
| ------------------------------------- | -------------------------------------------------------------- |
| `wait_index: true`                    | Block until the running album index settles (on `completed`)   |
| `write_file: <path>` + `content`      | Write UTF-8 text to a path                                     |
| `duplicate_of: {source, destination}` | Copy a file's bytes                                            |
| `chmod: {path, octal}`                | Change a path's POSIX permissions, `octal` in `chmod` spelling |

`wait_index` accepts only `true`: a scan that settles in any other state
panics the harness while it waits. To damage a file _before_ the indexer sees
it, use the `given:`-phase byte transforms `truncate_file`/`patch_file` —
`write_file` writes UTF-8 text, which matches no media signature and is
therefore skipped before any file is ever matched. `chmod` is how a scenario
watches the server meet a filesystem that refuses a read or a write (Unix
only).

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
| `click.select_first: true`                | Click the first grid image's hover action icon (opens the batch menu)         |
| `click.testid: <id>`                      | Click element by Playwright `data-testid`                                     |
| `fill: <role>/<label>, value: <value>`    | Type into an input                                                            |
| `select: <role>/<label>, option: <label>` | Choose from listbox/select                                                    |
| `submit`                                  | Submit the current form                                                       |
| `keyboard: <key>`                         | Press a key (e.g. `Tab`, `Escape`)                                            |
| `browser.back: true`                      | Browser history back                                                          |
| `wait.ms: <milliseconds>`                 | Pause execution (use sparingly — prefer auto-waiting assertions)              |
| `upload.files: {trigger, files}`          | Click `trigger` to open the file chooser, then attach `files`                 |
| `set.auto_rename: <bool>`                 | Set the auto-rename switch in the upload-options dialog                       |

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
`frontend/tests/playwright/schema.json` (UI). A schema mismatch is a hard error
in both.

**API scenarios are validated by a test.**
`backend/src/tests/scenario_schema.rs` compiles `backend/tests/schema.json` as
the draft it declares (`2020-12`) and validates every YAML file under
`backend/tests/scenarios/`, `selftest/` included, against it, under
`cargo test -p picasu`. The YAML is parsed as data and never executed, so the
check is static and costs no backend process. Three further tests keep the
schema honest rather than merely present: one checks it compiles as the dialect
it declares; one feeds it documents it must reject, so "every scenario
validates" cannot be satisfied by a schema that accepts everything; one
asserts the response-assertion forms listed above stay expressible, so a
tightened schema fails instead of the corpus.

`build.rs` does **not** validate the scenarios. It enumerates
`tests/scenarios/*.yaml` and `tests/scenarios/selftest/*.yaml` to generate one
`#[test]` per file, and the interpreter reads each file at run time.

The API check is a lower bound on the interpreter's acceptance, not an equality:
a form the schema allows and the interpreter silently ignores — a misspelled
`then:` key, for instance — passes validation. That class of gap is pinned by a
scenario instead, in `backend/tests/scenarios/selftest/`.

UI scenarios are validated at runtime by `loadScenarios.ts` via the Zod
`UiScenario` schema in `types.ts`.

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
