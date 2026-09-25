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
undocumented mounted route, a documented operation that is no longer mounted, or
a duplicate `operationId`.

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
   - Splits each `routes![]` block on commas, so a single-line
     `routes![a, b]` registers both handlers.
   - For each handler, reads its source file to check for a
     `#[utoipa::path]` annotation.
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
   the mounted Rocket routes with the public spec.

6. **`route_scan` tests** (`cargo test --lib route_scan`) — cover the scanner in
   `backend/build/route_scan.rs`, which `build.rs` shares with the test module so
   the `routes![]` parsing is testable outside the build script.

7. **`committed_artifact_is_up_to_date`** — asserts `public_json()` equals the
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

| Tag      | Routes                      | Description                                                            |
| -------- | --------------------------- | ---------------------------------------------------------------------- |
| _(none)_ | Standard data API endpoints | `GET /get/...`, `POST /post/...`, `PUT /put/...`, `DELETE /delete/...` |
| `pages`  | SPA HTML page routes        | `GET /albums`, `GET /login`, etc. — serve `index.html`                 |
| `albums` | Album assignment            | `PUT /put/assign_album`                                                |

## Workflow

### Adding a new data API route

1. Add the handler function to a `routes![]` block. If the handler lives in a
   module that is not scanned by `build.rs`, add that module to
   `collect_all_routes` — otherwise the route is mounted but undocumented.
2. Add `#[utoipa::path(...)]` with the route's HTTP method, path, parameters,
   and response types. The annotated `path` must match the mounted route
   exactly; the parity test fails on a mismatch. Pick the appropriate tag (or
   omit for standard data APIs).
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
| `build.rs`                              | —                   | Route scanner + `openapi.rs` generator + coverage  |
