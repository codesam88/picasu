# OpenAPI Generator Pipeline

## Motivation

The backend exposes 61 documented operations across GET, POST, PUT, and DELETE
modules, plus the test-only probes and the static file server. Keeping the API
documentation in sync with the actual implementation is a constant drift problem
in any live codebase.

This project avoids that drift by making the code itself the sole authority:

- **`routes![]` macros** are the single source of truth for _which routes exist_.
- **`#[utoipa::path]` annotations** are the single source of truth for _which
  routes have OpenAPI specs_.

From these two inputs, the pipeline generates the OpenAPI schema, a human-readable
API reference, and coverage metrics — eliminating any separate list that could
fall out of sync.

Neither input is the runtime route table, though, so two further checks close the
loop. `backend/src/tests/openapi_contract.rs` compares the routes Rocket actually
mounts against the operations in the public spec at runtime, and fails on an
undocumented mounted route, a documented operation that is no longer mounted, a
duplicate `operationId`, or a tag-taxonomy violation (an operation with no tag, a
tag outside the known set, or `pages` on a data-API operation). The
`openapi-sanity` CLI compares the annotated source with the committed document
without compiling anything, and fails on the failures the runtime table cannot
show: an annotation whose path or verb disagrees with the route attribute it sits
on, a handler registered twice, or a committed document that no longer matches
what the source declares.

The goal is an exact, auditable mapping between:

1. Available implemented API
2. Documentation reference
3. Checked-in public spec artifact

## Pipeline

```
┌─────────────────────┐     ┌──────────────────────────┐
│  routes![] macros   │     │  #[utoipa::path(...)]    │
│  (which routes)     │     │  (OpenAPI metadata)      │
└──────┬──────────────┘     └──────────┬───────────────┘
       │                               │
       ▼                               ▼
┌──────────────────────────────────────────────────────┐
│   build.rs + utils/openapi-sanity (every build)      │
│                                                      │
│   1. Scan all routes![] for handler names            │
│   2. Scan handler source for #[utoipa::path]         │
│   3. Warn on any missing annotations                 │
│   4. Write backend/src/openapi.rs                    │
└──────────────┬───────────────────────────────────────┘
               │
                ▼
┌────────────────────────────────────────────────────┐
│        just openapi-gen                            │
│   (cargo run -- --dump-openapi > openapi.json)     │
└──────┬──────────────────────────────┬──────────────┘
       │                             │
       ▼                             ▼
┌───────────────────────┐   ┌────────────────────────┐
│ backend/openapi.json  │   │ docs/openapi-reference │
│ (checked-in spec)     │   │ (widdershins markdown) │
└──────────┬────────────┘   └────────────────────────┘
           │
           ▼
┌──────────────────────────────────────────────────────┐
│ just openapi-check (part of just check, runs in CI)   │
│                                                      │
│ Phase 1  openapi-sanity check                        │
│          source vs. backend/openapi.json             │
│ Phase 2  regenerate and diff the committed artifact  │
└──────────────────────────────────────────────────────┘
```

### Steps

1. **`build.rs`** (automatic on every `cargo build`) — the build script:
   - Parses every `routes![]` invocation in `router/{get,post,put}/mod.rs`,
     `router/delete.rs` and `router/auth.rs` to discover every registered
     handler. A module missing from that list has its routes mounted but
     undocumented, which the parity test reports.
   - For each handler, checks whether _its own_ function carries a
     `#[utoipa::path]` annotation. A file that annotates one handler does not
     annotate its neighbours, so the check is per function.
   - Prints `cargo:warning=` for any handler missing an annotation, and for
     anything the scanner could not parse.
   - Writes `backend/src/openapi.rs` with the correct `__path_*` imports and
     `paths(...)` registration.

   The scanning itself lives in `utils/openapi-sanity`, not in the build script:
   a build script cannot be unit-tested in place, and a build script is the one
   place a parsing mistake hides. `build.rs` keeps only what needs the
   filesystem — reading the router files, deciding which of them to scan, writing
   the generated files — and takes the `routes![]` entries, the per-function
   annotations and the Rocket-to-`OpenAPI` path translation from the shared
   crate.

2. **`just openapi-gen`** — runs the `picasu` binary with `--dump-openapi`
   (`cargo run -- --dump-openapi > backend/openapi.json`), which serves
   `openapi_public::public_json()`: the generated document minus the test-only
   probes, pretty-printed with sorted keys.

3. **`just docs-openapi`** — chains `openapi-gen` with:
   - `widdershins` to convert `openapi.json` → `docs/openapi-reference.md`
   - `prettier` for consistent markdown formatting

4. **`just openapi-check`** — the API contract gate, in two phases (see
   [The contract gate](#the-contract-gate)). It is part of `just check`, so it
   runs in CI, and the pre-commit hook runs it for any commit that touches
   `backend/`.

5. **`openapi_contract` tests** (`cargo test --lib openapi_contract`) — compare
   the mounted Rocket routes with the public spec, and enforce the tag
   taxonomy below (every operation carries a known tag, `pages` sits on the
   SPA page routes and on nothing else). Both sides of the comparison are
   normalized with `openapi_sanity::to_spec_path`, the single path translation
   the crate owns.

6. **`openapi-sanity` tests** (`cargo test -p openapi-sanity`) — cover the
   scanner itself: `routes![]` entries in every layout, per-function annotation
   attribution, Rocket route attributes and their URIs, the path translation and
   the diagnostics for malformed input. The `route_scan` tests in the backend
   (`cargo test --lib route_scan`) cover what only the backend can answer: which
   router files are scanned, that each of them exists, that its group prefix
   matches its layout, and that the build script scans them through the shared
   crate rather than a private copy.

7. **`committed_artifact_is_up_to_date`** — asserts `public_json()` equals the
   committed `backend/openapi.json`, so a stale artifact fails `cargo test` as
   well as `just openapi-check`.

## The contract gate

`just openapi-check` is the single command developers and CI run for the API
contract. It is two phases, and a phase that fails stops the recipe with a
nonzero exit, so the failing phase's own diagnostics are what the run shows:

1. **`openapi-sanity check`** — the semantic phase. Compares the annotated source
   with the committed `backend/openapi.json` without compiling the backend, so it
   runs in about a second and needs no build artifacts.
2. **`openapi-artifact`** — the generated-artifact phase. Regenerates the spec
   with `cargo run --package picasu -- --dump-openapi` into a temporary file and
   fails when it differs from the committed `backend/openapi.json`, printing the
   diff and the fix.

The order matters only for the report: phase 1 is cheap and names the source that
has to change, so it runs before the phase that has to compile the backend.

### What the semantic phase checks

`openapi-sanity check` reads the router sources listed in
`openapi_sanity::SCANNED_MODULES` — the same list `build.rs` uses, so the
generator and the gate cannot disagree about which files make up the API — plus
every file those modules' `routes![]` blocks name a handler in. It reports, one
per line as `file:line: message`:

| Finding                                                                          | What it means                                                           |
| -------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `registered in routes![] but the function carries no #[utoipa::path] annotation` | A mounted route with no operation of its own                            |
| `the route serves X but its #[utoipa::path] declares Y`                          | The route URI and the annotated path disagree after normalization       |
| `the route declares GET but its #[utoipa::path] declares POST`                   | The route attribute and the annotated verb disagree                     |
| `GET X is declared in source but absent from the spec`                           | A handler the generator would register that the document does not carry |
| `GET X is in the spec but no scanned route declares it`                          | A committed operation no scanned source backs                           |
| `registered in routes![] more than once (first at file:line)`                    | A handler identity registered twice                                     |
| `duplicate operationId \`id\` claimed by A, B`                                   | Two operations share an id, which merges their generated client methods |

Malformed input is reported rather than guessed at, on the same stream: an
unparsable `routes![]` entry, a route attribute without a string-literal URI, and
a syntax error all come from the analyzer the build script uses.

A handler whose annotation disagrees with its own route attribute is reported
once, locally, and is not also reported as document drift. The document is
generated from the annotation, so it inherits the disagreement; a second finding
would only restate the first.

### The CLI

```
openapi-sanity check [options]
openapi-sanity help

--router-root <dir>     router source root        (default: backend/src/router)
--spec <file>           committed document        (default: backend/openapi.json)
--module <group>=<path> router module to scan, relative to <router-root>;
                        repeatable, replaces the built-in module list
--exclude-prefix <path> operation path prefix deliberately outside the public
                        contract; repeatable
```

Exit codes: `0` nothing to report, `1` contract findings (one per line on
stderr, summary on stderr as well), `2` unusable input — a missing module, an
unreadable or non-JSON document, a document with no `paths` object, or a usage
error.

The run is deterministic: no timestamps, no network, no environment beyond the
working directory used to shorten labels, and findings sorted by file, line and
message. Two runs over one tree print byte-identical output, which is what makes
the report reviewable in a diff.

`--exclude-prefix` is where the backend's deliberate exceptions live. The public
artifact strips the test-only probe surface (`/get/test/`) while the handlers stay
in the source, so `just openapi-check` passes `--exclude-prefix /get/test/`. The
rule is a CLI argument rather than a constant in the analyzer because it belongs
to whoever owns the artifact; a caller that forgets it sees the omission as two
findings rather than a passing gate.

### Testing the gate

`cargo test -p openapi-sanity` covers the gate itself:

- `tests/contract.rs` drives the checks over the fixture router trees in
  `tests/fixtures/`. `clean/` reports nothing; `drift/` carries one instance of
  every failure mode, and each rule is asserted as an exact diagnostic — file,
  line and message — plus one assertion over the whole report, so a rule that
  stopped reporting, or started reporting twice, is a test failure.
- `tests/cli.rs` covers what the library does not own: the argument handling, the
  exit codes, one finding per line on stderr, and two runs printing the same
  report. It also runs the gate over the real `backend/src/router` and
  `backend/openapi.json` and requires them to be clean today, so the gate cannot
  be neutered and stay green on the repository.

The backend's contract gates have the same property: each comparison there is a
pure function exercised with deliberately drifted inputs, so a refactor that
empties a check fails a named test instead of quietly passing. See
`.plan/openapi-contract-hardening.md` for the remaining contract work.

The markdown reference is generated but not drift-checked: `widdershins` is
fetched with `npx --yes` at generation time, which needs network access that CI
gates should not depend on. Regenerate it with `just docs-openapi` when the spec
changes; it therefore lags `backend/openapi.json` until someone does.

### Coverage check

Coverage is checked automatically by `build.rs` during every build. If a route
lacks `#[utoipa::path]`, the build prints a `cargo:warning=` for each missing
handler. No module-level exemptions exist — every route is subject to the check.

## Tag conventions

Every operation carries exactly one tag from this table, set as
`tag = "..."` in its `#[utoipa::path]` annotation. The tag is what the
generated reference groups operations by, so the vocabulary stays small and
subject-oriented.

| Tag        | Subject                                                                             | Example                            |
| ---------- | ----------------------------------------------------------------------------------- | ---------------------------------- |
| `auth`     | Authentication and token renewal                                                    | `POST /post/authenticate`          |
| `albums`   | Albums and shares: creation, assignment, covers, titles, descriptions, share links  | `PUT /put/assign_album`            |
| `assets`   | Per-asset metadata and editing: flags, rating, tags, rotation, thumbnails, deletion | `PUT /put/edit_tag`                |
| `config`   | Server configuration: read, write, password, export/import, path completion         | `PUT /put/config`                  |
| `index`    | Filesystem indexing jobs and full rebuild                                           | `POST /post/index/album`           |
| `serving`  | Media byte delivery (compressed and original files)                                 | `GET /object/imported/{file_path}` |
| `timeline` | Grid/list data: prefetch, rows, scrollbar, tag list, export                         | `GET /get/get-data`                |
| `upload`   | File upload                                                                         | `POST /upload`                     |
| `pages`    | SPA HTML page routes served from `router/get/get_page.rs` (serve `index.html`)      | `GET /login`                       |

`pages` is reserved: every SPA page route must carry it, and no data-API
operation may. The gate is `every_operation_carries_a_known_tag` in
`backend/src/tests/openapi_contract.rs`, whose `KNOWN_TAGS` constant must be
extended by one line (along with a row here) when the taxonomy grows. The
`pages` expectation is derived from path shape — data-API prefixes versus
everything else — rather than a hardcoded list of page paths; see the comment
on `is_data_api_path` for the assumption that makes that derivation valid.

## Workflow

### Adding a new data API route

1. Add the handler function to a `routes![]` block. If the handler lives in a
   module that is not scanned, add that module to `SCANNED_MODULES` in
   `utils/openapi-sanity/src/modules.rs` — otherwise the route is mounted but
   undocumented.
2. Add `#[utoipa::path(...)]` with the route's HTTP method, path, parameters,
   and response types. The annotated `path` and verb must match the mounted
   route exactly; `just openapi-check` phase 1 fails on a mismatch. Set
   `tag = "..."` to the subject from the Tag conventions table — every operation
   must carry one, and the tag gate fails on a missing or unknown tag.
3. Run `just openapi-gen` and `just docs-openapi` to regenerate the spec
   artifact and the reference.
4. Run `cargo test --lib openapi_contract` and `just openapi-check`.
5. Commit the handler, its annotation, and the regenerated spec together.

CI enforces steps 4 and 5: `just check` compares the source with the committed
spec, diffs the spec artifact, and the parity tests fail on undocumented or
stale operations.

### Removing a route

Delete the handler and its entry from `routes![]`. Run `just openapi-gen` and
`just docs-openapi`. The route disappears from the spec automatically, and the
parity test fails if the annotation was left behind.

### Changing a route's signature

Update the `#[utoipa::path(...)]` annotation. Run `just openapi-gen` and
`just docs-openapi`. `just openapi-check` fails until the committed spec matches
the annotation, so the change cannot be merged undocumented.

## Key Design Decisions

### Why generate `openapi.rs` instead of maintaining it manually?

The original approach required manually importing every `__path_*` symbol and
listing every handler in `paths(...)`. This was error-prone and duplicated what
`routes![]` already declares. The generator eliminates this maintenance burden
while guaranteeing completeness.

### Why generate `openapi.rs` in `build.rs` instead of xtask?

`#[derive(OpenApi)]` references `__path_*` items generated by
`#[utoipa::path(...)]` proc-macros in the same crate. Cross-crate access would
require re-exporting every `__path_*` symbol from `backend`'s public API — more
boilerplate, not less. `build.rs` runs before compilation and writes the file
into the source tree (`.gitignore`d, never committed), keeping the generated
code in the crate where it belongs. The serialized spec is different: it is
committed, because it is the reviewed artifact the CI gate diffs against.

### Why not gate utoipa behind a feature flag?

The `#[utoipa::path]` annotations are part of the route handler definitions and
don't affect runtime behavior when the OpenAPI spec isn't generated. The
original feature-gated approach added 53 `#[cfg_attr]` wrappers across 23 files
for negligible binary-size benefit. Making utoipa a standard dependency removed
all of them, simplifying the code and the build.

### Why the stack size override?

`ApiDoc::openapi()` builds the complete schema tree at runtime. With many
registered schemas and deeply nested types (AbstractData's three flattened
variants), the recursive traversal exceeds Linux's default 2 MB thread stack.
`RUST_MIN_STACK=16777216` is set in the `justfile` for the `openapi-gen`
recipe. Only the generation step needs it — normal backend operation is
unaffected.

### Why no explicit `components(schemas(...))`?

utoipa automatically registers any type referenced as a request or response body
in a `#[utoipa::path]` annotation, including all transitively reachable types.
The explicit schema list was redundant and has been removed.

### Why is there a source-level check as well as the runtime parity test?

They answer different questions, and the source check cannot replace the runtime
one. The parity test in `openapi_contract.rs` asks what Rocket _mounts_, which is
the only view that can see a route table assembled under a `cfg` feature, and it
costs a compiled test binary. The `openapi-sanity` CLI asks whether the
annotation agrees with the route attribute it is attached to — a question the
runtime route table cannot answer at all, since a route whose `#[utoipa::path]`
names a different path is mounted and served normally under the wrong name. It
runs on source, so it is cheap enough to sit in front of the artifact diff.

The split is deliberate: anything that needs to know what Rocket mounts stays in
the backend's integration tests, and the CLI stays a source analyzer.

### Why does the CLI take `--exclude-prefix` instead of knowing about probes?

The exclusion list is a property of the _artifact_, not of the analyzer: it says
which operations the public document is allowed to omit. The analyzer has no way
to know that, and hardcoding `/get/test/` in a shared crate would make it
backend-specific. Passing it from `just openapi-check` keeps the rule in the
reviewed recipe next to the artifact it describes, and a caller that forgets it
gets two loud findings rather than a silently narrowed contract.

## Files

| File                                    | Generator           | Role                                                                 |
| --------------------------------------- | ------------------- | -------------------------------------------------------------------- |
| `utils/openapi-sanity/src/`             | —                   | `syn`-based route/annotation scanner, path rules, source/spec checks |
| `utils/openapi-sanity/src/main.rs`      | —                   | `openapi-sanity check` CLI (phase 1 of the gate)                     |
| `backend/src/openapi.rs`                | `build.rs`          | ApiDoc struct with all routes (gitignored)                           |
| `backend/openapi.json`                  | `ApiDoc::openapi()` | Public OpenAPI 3.1 spec (committed, drift-checked)                   |
| `docs/openapi-reference.md`             | widdershins         | Human-readable API reference                                         |
| `backend/src/tests/openapi_contract.rs` | —                   | Mounted-route / spec parity gate                                     |
| `backend/build.rs`                      | —                   | Reads the router files, writes `openapi.rs`                          |
