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

Neither input is the runtime route table, though, so a third check closes the
loop: `backend/src/tests/openapi_contract.rs` compares the routes Rocket
actually mounts against the operations in the public spec, and fails on an
undocumented mounted route, a documented operation that is no longer mounted, a
duplicate `operationId`, or a tag-taxonomy violation (an operation with no tag,
a tag outside the known set, or `pages` on a data-API operation).

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
│   build.rs (runs every build, no feature flag)       │
│                                                      │
│   1. Scan all routes![] for handler names            │
│   2. Scan handler source for #[utoipa::path]         │
│   3. Warn on any missing annotations                 │
│   4. Write backend/src/openapi.rs                         │
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
│   regenerate and diff against the committed artifact  │
└──────────────────────────────────────────────────────┘
```

### Steps

1. **`build.rs`** (automatic on every `cargo build`) — the build script:
   - Parses every `routes![]` invocation in `router/{get,post,put}/mod.rs`,
     `router/delete.rs` and `router/auth.rs` to discover every registered
     handler. A module missing from that list has its routes mounted but
     undocumented, which the parity test reports.
   - Parses each `routes![]` macro's token stream out of the file's AST (in
     `backend/build/ast_scan.rs`), so a single-line `routes![a, b]` registers
     both handlers and an entry that is not a plain `handler` or
     `module::handler` path is reported instead of guessed at.
   - Parses each handler's source file and checks _that function_ for a
     `#[utoipa::path]` annotation — per function, not per file — and warns
     when the annotation's `path = "..."` disagrees with the Rocket
     attribute's URI after the shared `to_spec_path` translation in
     `backend/build/route_path.rs`, which the parity gate also uses, so the
     two comparisons cannot drift apart.
   - Prints `cargo:warning=` for any handler missing an annotation.
   - Writes `backend/src/openapi.rs` with the correct `__path_*` imports and
     `paths(...)` registration.

2. **`just openapi-gen`** — runs the `picasu` binary with `--dump-openapi`
   (`cargo run -- --dump-openapi > backend/openapi.json`), which serves
   `openapi_public::public_json()`: the generated document minus the test-only
   probes, pretty-printed with sorted keys.

3. **`just docs-openapi`** — chains `openapi-gen` with:
   - `widdershins` to convert `openapi.json` → `docs/openapi-reference.md`
   - `prettier` for consistent markdown formatting

4. **`just openapi-check`** — regenerates the spec into a temporary file and
   fails when it differs from the committed `backend/openapi.json`, printing the
   diff and the fix. It is part of `just check`, so it runs in CI, and the
   pre-commit hook runs it for any commit that touches `backend/`.

5. **`openapi_contract` tests** (`cargo test --lib openapi_contract`) — compare
   the mounted Rocket routes with the public spec, and enforce the tag
   taxonomy below (every operation carries a known tag, `pages` sits on the
   SPA page routes and on nothing else).

6. **`ast_scan` tests** (`cargo test --lib ast_scan`) — cover the AST analysis in
   `backend/build/ast_scan.rs`, which `build.rs` shares with the test module so
   the `routes![]` parsing, the per-function annotation check and the attribute
   path-agreement check are testable outside the build script.

7. **`route_path` tests** (`cargo test --lib route_path`) — cover the shared
   Rocket→OpenAPI translation in `backend/build/route_path.rs`, including the
   test that pins its two call sites to the same behaviour: every Rocket path
   declared in the router (derived with the `ast_scan` pass) must translate to
   a path in the committed `backend/openapi.json`.

8. **`committed_artifact_is_up_to_date`** — asserts `public_json()` equals the
   committed `backend/openapi.json`, so a stale artifact fails `cargo test` as
   well as `just openapi-check`.

The gate logic has negative self-checks: each comparison is a pure function
exercised with deliberately drifted inputs, so a refactor that empties a check
fails a named test instead of quietly passing. See
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
   module that is not scanned by `build.rs`, add that module to
   `collect_all_routes` — otherwise the route is mounted but undocumented.
2. Add `#[utoipa::path(...)]` with the route's HTTP method, path, parameters,
   and response types. The annotated `path` must match the mounted route
   exactly; the parity test fails on a mismatch. Set `tag = "..."` to the
   subject from the Tag conventions table — every operation must carry one,
   and the tag gate fails on a missing or unknown tag.
3. Run `just openapi-gen` and `just docs-openapi` to regenerate the spec
   artifact and the reference.
4. Run `cargo test --lib openapi_contract` and `just openapi-check`.
5. Commit the handler, its annotation, and the regenerated spec together.

CI enforces steps 4 and 5: `just check` diffs the spec artifact, and the
parity tests fail on undocumented or stale operations.

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

## Files

| File                                    | Generator           | Role                                               |
| --------------------------------------- | ------------------- | -------------------------------------------------- |
| `backend/src/openapi.rs`                | `build.rs`          | ApiDoc struct with all routes (gitignored)         |
| `backend/openapi.json`                  | `ApiDoc::openapi()` | Public OpenAPI 3.1 spec (committed, drift-checked) |
| `docs/openapi-reference.md`             | widdershins         | Human-readable API reference                       |
| `backend/src/tests/openapi_contract.rs` | —                   | Mounted-route / spec parity gate                   |
| `backend/build/ast_scan.rs`             | —                   | AST route/handler analysis shared with unit tests  |
| `backend/build/route_path.rs`           | —                   | Shared Rocket→OpenAPI path translation             |
| `build.rs`                              | —                   | AST route scan + `openapi.rs` generator + coverage |
