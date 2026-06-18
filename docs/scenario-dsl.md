# Scenario DSL

Semi-formal spec and authoring guide for spec-driven E2E testing.

Two scenario types share the `given:` vocabulary but have disjoint
`when:`/`then:` verb sets. Place scenario files by type:

- `scenarios/api/*.yaml` — backend-only Rocket-`Client` tests (generated Rust)
- `scenarios/ui/*.yaml` — Playwright browser tests (generated TypeScript)

## Common structure

Every scenario file is a YAML document with three top-level keys:

```yaml
name: Human-readable name for the scenario
given:
  - ... # fixture definitions
when: ... # verb set depends on scenario type (directory)
then: ... # verb set depends on scenario type (directory)
```

## `given:` vocabulary (shared)

Each entry in `given:` calls a fixture builder and optionally binds the
result to an `id_as` variable for later reference in `when:` bodies and
`then:` assertions. Variables are interpolated as `${variable_name}` in
string values.

| Form                | Description                                                     | Maps to                                                                               |
| ------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `dir_album: <path>` | Create a dir album at `path`, return its album ID               | `make_dir_album(&Path::new(path))`                                                    |
| `photo: <path>`     | Insert a photo record (no real file) at `path`, return its hash | `insert_photos(&[PhotoSpec { path, tags, exif_date }])` + `find_hash_by_alias_path()` |
| `tag: <name>`       | Create a tag (typically combined with `photo`)                  | passed into `PhotoSpec.tags`                                                          |
| `empty`             | No-op, ensures `TEST_ENV` is initialised                        | `let _ = &*TEST_ENV;`                                                                 |

Optional modifier fields per `given:` entry:

- `id_as: <name>` — binds the result to `${name}`
- `tags: [<tag>, ...]` — for `photo`, sets the photo's tags
- `exif_date: <string>` — for `photo`, sets `DateTimeOriginal`
- `color: [<r>, <g>, <b>]` — for real (decoded) photo fixtures, sets pixel colour

## API scenarios (`scenarios/api/`)

These expand into Rocket-`Client` Rust tests in `fixtures.rs` style.

### `when:` — single API call

```yaml
when:
  call: <method> <path> # e.g. "PUT /put/assign_album"
  body: <json-value> # request body; `${var}` interpolation
  auth: <true|false> # default true; attaches admin auth cookie
```

`call` is resolved against `openapi.json` for operation existence — the
generator validates that the method+path pair exists and that `body` matches
the request schema.

### `then:` — assertions (one or more)

| Form                            | Assertion                     |
| ------------------------------- | ----------------------------- |
| `response.status: <code>`       | HTTP status code              |
| `response.<json-path>: <value>` | JSON body field matches value |
| `response.<json-path] exists`   | JSON body field is present    |
| `response.<json-path] absent`   | JSON body field is absent     |
| `file_exists: <path>`           | File exists on disk           |
| `file_absent: <path>`           | File does not exist on disk   |

`<json-path>` is a dot-separated path into the response JSON, e.g.
`prefetch.locateTo` or `prefetch.timestamp`.

### Escape-hatch policy (API)

No raw-Rust escape hatch for assertions. A missing assertion form is
resolved by adding a reusable verb to the vocabulary, not by inlining
code. If a scenario needs a multi-step interaction (e.g. call A, then
call B using state from A), use multiple scenarios or a multi-step
`when:` block:

```yaml
when:
  - call: PUT /put/assign_album
    body: { hash: "${photo}", album_id: "${album}" }
    capture: response
  - call: GET /get/get-albums
    auth: true
```

## UI scenarios (`scenarios/ui/`)

These expand into Playwright TypeScript specs driving a real backend +
built frontend. No API-call verbs, no response assertions — only user
interactions and visible UI state.

### `given:` seeding

Any working endpoint or redb-direct call — UI assertions only check visible
state, so a seed crash is a setup failure, not a silent false pass. The same
`given:` vocabulary as API scenarios applies.

### `when:` — user interactions (ordered list)

Elements are referenced by **ARIA role** and **accessible name** (never CSS
selectors).

| Verb                                      | Description                                    |
| ----------------------------------------- | ---------------------------------------------- |
| `navigate: <route>`                       | Go to a URL pattern (e.g. `/`, `/albums/<id>`) |
| `click: <role>/<label>`                   | Click element by ARIA role + accessible name   |
| `fill: <role>/<label>, value: <value>`    | Type into an input                             |
| `select: <role>/<label>, option: <label>` | Choose from listbox/select                     |
| `submit:`                                 | Submit the current form                        |

New interactions → extend the vocabulary with a new verb. No raw-TypeScript
escape hatch.

### `then:` — UI assertions (one or more)

| Form                                        | Assertion                                                            |
| ------------------------------------------- | -------------------------------------------------------------------- | ------------------ |
| `ui.visible: <role>/<label>`                | Element is visible                                                   |
| `ui.hidden: <role>/<label>`                 | Element is hidden/absent                                             |
| `ui.text: <role>/<label>, contains: <text>` | Element text includes string                                         |
| `ui.toast: type: <type>, contains: <text>`  | Toast of given type (`error`/`success`/`warning`) with matching text |
| `ui.modal: open                             | closed`                                                              | Modal dialog state |
| `ui.route: <pattern>`                       | Current URL matches pattern                                          |
| `ui.aria_snapshot: <name>`                  | Compare ARIA role/name/state tree against committed `.aria` snapshot |

### Escape-hatch policy (UI)

Same as API: no raw-TypeScript. A missing interaction or assertion verb is
a gap in the DSL, not a reason to inline code. Extend the vocabulary in
`docs/scenario-dsl.md` and the generator's verb parser when a new form is
needed.

## Schema validation

The DSL has its own JSON Schema at `scenarios/schema.json`. All scenario
files are validated structurally against this schema before generation.
A scenario that fails schema validation is a hard error — it never produces
a green test.

```bash
# Validate all scenario files (via xtask)
cargo xtask test-backend
```

## Idempotency and isolation

- Each scenario creates its own fixtures with unique, namespaced paths
  (use the scenario name as a prefix, e.g. `/e2e_h_*` for Scenario H).
- `given:` entries that reference `dir_album` without a pre-existing photo
  create the album record only; photos must be added separately.
- In API scenarios, all assertions are made through the HTTP API only — no direct redb access.
- In UI scenarios, state is seeded before the browser navigates to the
  page under test.
