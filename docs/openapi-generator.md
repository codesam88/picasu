# OpenAPI Generator Pipeline

## Motivation

The backend exposes ~63 route handlers across GET, POST, PUT, and DELETE
modules. Keeping the API documentation and test coverage in sync with the
actual implementation is a constant drift problem in any live codebase.

This project avoids that drift by making the code itself the sole authority:

- **`routes![]` macros** are the single source of truth for _which routes exist_.
- **`#[utoipa::path]` annotations** are the single source of truth for _which
  routes have OpenAPI specs_.

From these two inputs, the pipeline generates the OpenAPI schema, a human-readable
API reference, and coverage metrics — eliminating any separate list that could
fall out of sync.

The goal is an exact, auditable mapping between:

1. Available implemented API
2. Documentation reference
3. Complete API testing based on generated coverage metrics

This minimises the risk of undocumented or untested functions due to code drift.

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
┌──────────────┐      ┌──────────────────────────┐
│ openapi.json │      │ docs/openapi-reference.md │
│ (spec)       │      │ (widdershins markdown)    │
└──────────────┘      └──────────────────────────┘
```

### Steps

1. **`build.rs`** (automatic on every `cargo build`) — the build script:
   - Parses every `routes![]` invocation in `router/{get,post,put}/mod.rs` and
     `router/delete.rs` to discover every registered handler.
   - For each handler, reads its source file to check for a
     `#[utoipa::path]` annotation.
   - Prints `cargo:warning=` for any handler missing an annotation.
   - Writes `backend/src/openapi.rs` with the correct `__path_*` imports and
     `paths(...)` registration.

2. **`just openapi-gen`** — runs the `picasu` binary with `--dump-openapi`
   (`cargo run -- --dump-openapi > backend/openapi.json`), which calls
   `ApiDoc::openapi().to_json()` and writes the result to the file.

3. **`just openapi-docs`** — chains `openapi-gen` with:
   - `widdershins` to convert `openapi.json` → `docs/openapi-reference.md`
   - `prettier` for consistent markdown formatting

4. **`just openapi-docs-check`** — runs the full generation, then fails if
   `openapi.json` or `docs/openapi-reference.md` differs from the committed
   versions. Wired into `just precommit` on the `main` branch.

### Coverage check

Coverage is checked automatically by `build.rs` during every build. If a route
lacks `#[utoipa::path]`, the build prints a `cargo:warning=` for each missing
handler. No module-level exemptions exist — every route is subject to the check.

## Tag conventions

| Tag | Routes | Description |
|---|---|---|
| *(none)* | Standard data API endpoints | `GET /get/...`, `POST /post/...`, `PUT /put/...`, `DELETE /delete/...` |
| `pages` | SPA HTML page routes | `GET /home`, `GET /albums`, `GET /login`, etc. — serve `index.html` |
| `development` | Debug-only tooling | `GET /put/generate_random_data` — generates fake data for testing |

## Workflow

### Adding a new data API route

1. Add the handler function to a `routes![]` block.
2. Add `#[utoipa::path(...)]` with the route's HTTP method, path, parameters,
   and response types. Pick the appropriate tag (or omit for standard data
   APIs).
3. Run `just openapi-docs` — this regenerates `openapi.json` and the docs.
4. Run `cargo build` — build.rs confirms 100% coverage (no warnings).
5. Commit the handler, its annotation, and the two generated files together.

Pre-commit hook or CI enforces step 4 — if coverage drops below 100% the
build prints warnings.

### Removing a route

Delete the handler and its entry from `routes![]`. Run `just openapi-docs`.
The route disappears from all generated files automatically.

### Changing a route's signature

Update the `#[utoipa::path(...)]` annotation. Run `just openapi-docs`.
The spec and reference docs reflect the change immediately.

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
code in the crate where it belongs.

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

| File                             | Generator                 | Role                                                |
| -------------------------------- | ------------------------- | --------------------------------------------------- |
| `backend/src/openapi.rs`          | `build.rs`                | ApiDoc struct with all routes (gitignored)          |
| `backend/openapi.json`            | `ApiDoc::openapi()`       | OpenAPI 3.1 spec                                    |
| `docs/openapi-reference.md`      | widdershins               | Human-readable API reference                        |
| `build.rs`                       | —                         | Route scanner + `openapi.rs` generator + coverage   |
