# OpenAPI Generator Pipeline

## Motivation

The backend exposes 61 documented operations across GET, POST, PUT, and DELETE
modules, plus the test-only probes and the static file server. Keeping the API
documentation in sync with the actual implementation is a constant drift problem
in any live codebase.

This project avoids that drift by making the code itself the sole authority:
`#[utoipa::path]` annotations are the single source of truth for the documented
operations, and `#[utoipauto]` discovers them. From that one input the pipeline
generates the OpenAPI schema and a human-readable API reference — there is no
list of modules and no generated Rust file that could fall out of sync.

Neither input is the runtime route table, though, so one further check closes the
loop: `picasu --check-openapi`, a mode on the binary that builds the real
`build_rocket()` and compares its mount table with the committed document (see
[Route-set parity](#route-set-parity---check-openapi)). It fails on an
undocumented mounted route, and on a documented operation that no build mounts —
except one gated behind a feature this build does not have. That is the half no
external tool can do for us: a route mounted without documentation, or documented
without a route, is a failure.

`backend/src/tests/openapi_contract.rs` runs the same comparisons over a _test_
build's route table. They are the self-checks, not the gate: each comparison is a
pure function exercised with deliberately drifted inputs, so a refactor that
emptied one fails a named test instead of turning the gate into a no-op that
still reports success.

The goal is an exact, auditable mapping between:

1. Available implemented API
2. Documentation reference
3. Checked-in public spec artifact

## Pipeline

```
┌──────────────────────────────────────────────────────┐
│      #[utoipa::path(...)]  (the OpenAPI metadata)    │
│      on every handler under backend/src/router       │
└──────────────────────┬───────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────┐
│  #[utoipauto(paths = "./backend/src/router")]        │
│  on ApiDoc (macro expansion, every build)            │
│                                                      │
│   1. Walk backend/src/router recursively             │
│   2. Collect every __path_* it can reach              │
│   3. Inject them into #[openapi(paths(...))]          │
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
│ Phase 1  openapi-json-match: regenerate and diff     │
│ Phase 2  openapi-routes-match: --check-openapi       │
│          build_rocket().routes() vs the spec         │
└──────────────────────────────────────────────────────┘
```

### Steps

1. **`#[utoipauto]`** (macro expansion, on every `cargo build`) — declared in
   `backend/src/openapi.rs` directly above `#[derive(OpenApi)]`:

   ```rust
   #[utoipauto(paths = "./backend/src/router")]
   #[derive(OpenApi)]
   #[openapi(components(responses(Unauthorized)))]
   pub struct ApiDoc;
   ```

   One directory, walked recursively — there is no file list and no
   `from ... => ...` mapping. `mod.rs`, group-root files (`router/delete.rs`,
   `router/auth.rs`) and nested groups are all resolved from the file layout
   alone. Every function carrying `#[utoipa::path]` contributes its `__path_*`
   item to `paths(...)`; every struct deriving `ToSchema` or `ToResponse`
   contributes to `components(schemas(...))` / `components(responses(...))`.
   The `Unauthorized` response component is registered explicitly because it
   lives in `backend/src/openapi_components.rs`, outside the scanned tree.

   The directory is spelled relative to the **workspace root**, not the package,
   because that is the working directory cargo runs rustc in.

   One derivation is imperfect, and it is in the document rather than the code:
   the six SPA catch-all routes (`/albums/view/{path}` and its four siblings,
   plus the `/<path..>` fallback) document their trailing segment as a **required**
   path parameter, while `<path..>` matches zero or more segments — `/albums/view`
   is routed too. utoipa derives `required` from the handler argument's type, and
   a catch-all's argument is a `PathBuf` rather than an `Option`. Declaring the
   parameter inline (`params(("path" = Option<String>, Path))`) makes its schema
   nullable but leaves `required` at the derived value, so it is not a way out.
   The parameter exists only to give the catch-all something to bind: the handlers
   ignore it and serve `index.html`.

2. **`build.rs`** — no longer discovers anything. It exists for
   `generate_scenarios_rs`, which writes the YAML-driven test functions into
   `OUT_DIR`. `#[utoipauto]` reads the router files with `std::fs` while the
   macro expands, so rustc's dependency info never mentions them; no
   fingerprint or `rerun-if-changed` is needed for that, because every edit
   that matters — an annotation added or changed, a new module declared —
   changes a file the crate is being compiled from, so the crate is recompiled
   and the macro re-expands. A file added under `src/router` that no `mod`
   declares is not compiled at all, and the macro reports it as a compile error
   rather than ignoring it.

3. **`just openapi-gen`** — runs the `picasu` binary with `--dump-openapi`
   (`cargo run -- --dump-openapi > backend/openapi.json`), which serves
   `openapi_public::public_json()`: the generated document minus the test-only
   probes, pretty-printed with sorted keys.

4. **`just docs-openapi`** — chains `openapi-gen` with:
   - `widdershins` to convert `openapi.json` → `docs/openapi-reference.md`
   - `prettier` for consistent markdown formatting

5. **`just openapi-check`** — the API contract gate, in two phases (see
   [The contract gate](#the-contract-gate)). It is part of `just check`, so it
   runs in CI, and the pre-commit hook runs it for any commit that touches
   `backend/`.

6. **`openapi_contract` tests** (`cargo test --lib openapi_contract`) — compare
   the mounted Rocket routes with the public spec, check that `operationId`s are
   unique, that the exclusion list matches the routes it actually excludes, and
   that every documented `401` is a `$ref` to the shared `Unauthorized` component
   rather than an inlined literal. Both sides of the route comparison are
   normalized with `backend/src/spec_path.rs::to_spec_path`, the single path
   translation the backend owns. They read the _test_ build's route table; the
   route-set verdict belongs to `--check-openapi`, because a test build runs
   without `embed-frontend` and serves a different table than the one that ships
   (see [Route-set parity](#route-set-parity---check-openapi)).

7. **`openapi_parity` tests** (`cargo test --lib openapi_parity`) — the
   asymmetric parity rule over fixtures, since the feature-excuse branch has no
   instance in the repository: a mounted route the document omits fails, an
   ungated document-only operation fails, and a feature-gated operation absent
   from this build is excused — and is drift once the feature is on. The same
   file pins every `x-picasu-feature` value in the committed document to a
   feature `backend/Cargo.toml` declares.

8. **`committed_artifact_is_up_to_date`** — asserts `public_json()` equals the
   committed `backend/openapi.json`, so a stale artifact fails `cargo test` as
   well as `just openapi-check`.

## The contract gate

`just openapi-check` is the single command developers and CI run for the API
contract. It runs three phases, and a failure stops the recipe with a nonzero
exit, so the failing phase's own diagnostics are what the run shows:

1. **`openapi-sanity`** — the source phase. Runs
   [`utils/openapi-sanity`](../../utils/openapi-sanity/README.md) over
   `backend/src/router` and reports one `file:line: message` per problem in an
   annotation or in the handler it sits on. It comes first because the other two
   phases compare a document that has to be regenerated before either of them can
   say anything: a defect found after the diff is a confusing way to be told about
   it. The rules are [annotation shape](#annotation-shape-what-the-source-gate-checks)
   and the parameter-agreement rules; they read source only, so the phase needs
   no build and no frontend bundle.
2. **`openapi-json-match`** — the generated-artifact phase. Regenerates the spec
   with `cargo run --package picasu -- --dump-openapi` into a temporary file and
   fails when it differs from the committed `backend/openapi.json`, printing the
   diff and the fix.
3. **`openapi-routes-match`** — the route-set phase, `picasu --check-openapi` run
   against a build configured like the shipped one. This is the only check that
   can prove the route-set half of the invariant, and it is pinned to the release
   feature set (`--features "embed-frontend auto-open-browser"`, the set
   `.github/workflows/release.yml` builds) so the build doing the checking is the
   build that ships: a feature-gated route only exists in the table of a build
   that has the feature, so checking a build without it could not see the route
   at all. The phase depends on the frontend bundle because `embed-frontend`
   embeds it.

The same flag runs on the binary the release job ships
(`.github/workflows/release.yml`), so the shipped build is the one that was
checked.

The source phase runs with `--expect-at-least 60`, a floor on the annotated
handlers the scan must see. A file walk that stopped descending produces exactly
the report a clean tree produces, so a scan below the floor fails instead of
reporting clean. 60 is headroom below the tree's 63 annotations: a new handler
must not break the gate, a lost one should be noticed.

### Annotation shape (what the source gate checks)

Two of the three phases compare the document. The source phase checks the
annotations themselves, because a handful of things about an annotation are wrong
in a way the document cannot show:

| Rule | Assertion                                                                      | Why the document cannot show it                                                                                        |
| ---- | ------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------- |
| A1   | no `path = "…"` and no bare verb token in the annotation                       | `rocket_extras` derives both from the route attribute, so a restatement is a second copy of a fact nothing compares    |
| A2   | `responses(…)` is present and has at least one entry                           | utoipa invents no response, so an operation with none documents nothing it can answer                                  |
| A3   | exactly one `tag = "…"`, from the table in [Tag conventions](#tag-conventions) | a missing or unknown tag files the operation outside every section of the reference                                    |
| A4   | the handler carries a doc comment                                              | `summary` and `description` are derived from it, so a handler without one is a complete-looking operation with no text |
| A5   | the doc comment's first paragraph is one line                                  | it is the `summary`, and the reference renders the `summary` as a heading — a newline inside a heading splits it       |
| A6   | no `operation_id = "…"`                                                        | utoipa derives it from the function name; a hand-set one is the only name nothing compares                             |
| A7   | no `summary = "…"` and no `description = "…"`                                  | utoipa derives both from the doc comment; a hand-set one is the same prose written twice, with nothing comparing them  |
| B1   | every declared `params(…)` name is a `<segment>` or `?<name>` the route binds  | utoipa merges declared parameters into the derived document without checking that the route reads them                 |
| B2   | a declared parameter's documented `required` matches the handler argument      | utoipa derives `required` from the declared type and never looks at the argument the route binds                       |
| B3   | a declared `request_body` names the type the route's `data = "…"` parses       | utoipa takes the declared schema and never compares it to what Rocket parses                                           |
| B4   | a `Form<…>` binding declares `multipart/form-data`                             | utoipa guesses `application/json` for a named non-primitive type, so a multipart endpoint was published as a JSON one  |

`docs/openapi-generator.md` is where the tag vocabulary is written down, and
`utils/openapi-sanity/src/lib.rs` holds the tool's copy of it (`TAGS`); the two
are changed together. Everything else in the table is a rule with no
document-side equivalent, which is why they live in the tool and not in a linter
for the document — the document is generated _from_ these annotations, so a
linter would be checking the output against its own input.

Every rule reads what the annotation, the handler beside it, or the handler's own
route attribute says. None of them reads a route path, a config value, a feature
name or a constant from the backend, and that is deliberate: a rule that needs one
of those belongs in the backend or in a just recipe, where the fact is kept once.
B1–B4 read the route attribute on the handler they are already reading — the same
attribute utoipa reads it from — which is a fact in the file under test rather
than a backend fact copied into the tool.

B1 and B2 read a declared parameter's name, location and type, and they read the
inline tuple form `("name" = Type, Location, …)` only. The struct form —
`params(SomeQueryStruct)` — hides all three behind a type, and **no type in this
repository derives `IntoParams`**, so the tool counts those entries instead of
reading them and pins the count at zero: the first one to appear fails a test
rather than quietly narrowing the rules. B3 states two limits rather than
exempting itself from them: `request_body = Value` declares no constraint, and a
`Form<…>` payload has no schema type an annotation could name. An out-of-contract operation is
therefore not an exception to A3 but an entry in its vocabulary — `internal`, for
the test-only probes, whose operations
[`openapi_public`](../../backend/src/openapi_public.rs) strips from the committed
artifact. Every other rule applies to them too, because the contract tests read
the _full_ spec rather than the public one: their responses (A2) and their doc
comments (A4) are still read.

Three spellings utoipa also accepts are still review-time, and the boundary is
deliberate: `method(GET)` is the parenthesised verb form of A1, `tags([…])` is a
list form of A3, and `context_path` is a base-path form of A1. No annotation uses
any of them.

## Route-set parity (`--check-openapi`)

`--check-openapi` is the check that proves the route-set half of the invariant. It
is a mode on the binary, a sibling of `--dump-openapi`:

```
picasu --check-openapi [<path-to-spec>]   # default: backend/openapi.json
```

It reads the **committed** spec (the published claim, not the compiled-in copy),
builds the real `build_rocket()`, reads `.routes()`, normalizes both sides with
the backend's `to_spec_path`, and compares them under an **asymmetric** rule:

- **Mounted ⊆ spec** — every route the running product registers must be in the
  spec. A hard failure. This is the direction that cannot be argued with.
- **Spec ⊆ mounted, feature-gated** — a spec operation that is not mounted in
  _this_ build is acceptable when, and only when, it is feature-gated and that
  feature is disabled here. An ungated spec operation with no matching mount is
  still drift.

Before the comparison both sides are normalized and then **dropped** under
`openapi_public::CONTRACT_EXCLUSION_PREFIXES` — `/get/test/` and `/assets` — the
one backend-owned list of surfaces outside the published contract. The same const
is what `contract_exclusions_match_mounted_routes` holds to the routes it
actually matches, so a prefix cannot be added to one consumer alone.

Nothing under the exclusions is excused because of a feature: a mount with no
operation is a failure whatever the reason it is mounted. The asymmetry only
applies in the other direction, to operations the document carries.

The asymmetry is what feature-gating requires. The build's enabled features are
read with `cfg!` (`openapi_parity::enabled_features`), so the check runs
correctly in any configuration. It exits `0` when the two views agree, `1` with a
per-route report when they do not, and `2` when the document cannot be read or
parsed — a missing file names `just openapi-gen` rather than reporting a clean run.

The comparison is a pure function over three inputs — the mounted routes, the
document's operations, and this build's enabled features
(`openapi_parity::compare_route_set`) — so the rule is tested without a server
and without a product build, and the flag and the backend's parity self-checks run
the same code rather than two comparisons that can drift apart.

The `/get/test/` probes are registered only in test builds (a `#[cfg(test)]`
extension of `generate_get_routes()`), which is what the route-set rule requires:
the public document never carried them, so a shipped build that mounted them
would fail the gate. The handlers and their annotations stay compiled in every
build, because `build.rs` generation is cfg-blind and the spec keeps the probe
paths. `backend/tests/probe_registration.rs` observes that from an integration
test, which is the only vantage point outside `cfg(test)`.

### Feature-dependent routes

A single canonical `openapi.json` describes every route _any_ build can expose —
the union across features, not one build's slice. An operation that exists only
under a feature carries that feature as a vendor extension, set with utoipa's
`extensions(...)` on `#[utoipa::path]`:

```rust
#[utoipa::path(
    extensions(("x-picasu-feature" = json!("embed-frontend"))),
    ...
)]
```

The key has to be spelled whole: utoipa prefixes a bare key with `x-` and
otherwise leaves the rest as written, so `("picasu_feature" = …)` would serialize
as `x-picasu_feature` and the check would not see it. The marker is written by
the annotation author — `build.rs` cannot evaluate a `cfg` at a mount site.

`--check-openapi` reads that marker, so in a build without `embed-frontend` the
operation's absence from the mount table is expected rather than drift. This is
what lets a feature-gated API be documented without a hand-written exclusion —
exclusions remain only for genuinely non-API surfaces (static file mounts,
test-only probes).

A marker naming a feature `backend/Cargo.toml` does not declare would make the
`cfg!` read permanently false and excuse the operation in every build, so
`every_feature_marker_in_the_committed_spec_is_a_declared_feature` holds the
committed document to the manifest's `[features]`, and
`every_declared_feature_is_readable_by_the_enabled_feature_list` holds the list
the check asks `cfg!` about to the same set. No operation carries a marker today,
so both are fixture-backed rather than repository-backed.

The spec dependency of this check is deliberately confined to the repo, the commit
hook, CI and the release gate. `openapi.json` is a review artifact, not a
deployment dependency, so the server's boot does not depend on it and a missing
file cannot stop the server from starting.

## Tag conventions

Every operation carries exactly one tag, set as `tag = "..."` in its
`#[utoipa::path]` annotation. The tag is what the generated reference groups
operations by, so the vocabulary stays small and subject-oriented.

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
| `internal` | Operations outside the published API — the test-only probes                         | `GET /get/test/record/{asset_id}`  |

`pages` is reserved for the SPA page routes in `router/get/get_page.rs`; the data
API lives under `/delete/`, `/get/`, `/object/`, `/post/`, `/put/` and `/upload`
and takes a subject from the table.

This table **is** a checked rule: A3 of the source gate holds every annotation to
exactly one tag from it, and the gate fails the build on a tag that is not in the
list. The list the tool checks is `TAGS` in `utils/openapi-sanity/src/lib.rs` —
a copy of this table, not a parse of it, because the document is generated from
these annotations and reading the vocabulary back out of it would check the
output against its own input. **Adding a subject means changing this table and
that constant in the same change.**

`internal` names the operations **outside the published API**, which today are
the test-only probes under `/get/test/`. `openapi_public` strips those from the
committed artifact, along with the routes under the contract's exclusion
prefixes, so the generated reference never renders this group — the tag says "not
part of the published surface" rather than naming a section a reader can open.
Every operation is tagged, and `internal` is how an out-of-contract one says so;
nothing is exempt from the rule.

## Workflow

### Adding a new data API route

1. Add the handler function to a `routes![]` block, in a module under
   `backend/src/router` (declared from `router/mod.rs`). Every module in that
   tree is scanned, so nothing has to be registered anywhere else — but an
   annotated `.rs` file that no `mod` statement declares is a build error, since
   its `__path_*` item has no module to live in.
2. Add `#[utoipa::path(...)]` with the operation's responses, its `tag = "..."`
   from the table above, and `params(...)` for any parameter that needs a
   description or a schema the handler signature cannot carry. Leave the HTTP
   method and the path out: with `rocket_extras` enabled, utoipa reads both from
   the route attribute itself, so the document cannot name a path the route does
   not serve — and A1 fails the build if the annotation restates either, because
   a second copy of a route fact is a copy that can rot. Do not set
   `operation_id` (A6), and do not set `summary` or `description` (A7): utoipa
   derives all three, and a hand-set one is the same text written twice beside
   itself, with nothing comparing the copies. Declare at least one
   `responses(…)` entry (A2) — utoipa invents none. If the route is
   feature-gated, add
   `extensions(("x-picasu-feature" = json!("<feature>")))` so `--check-openapi`
   knows a spec operation may legitimately be absent from a build without it.
3. Write a doc comment on the handler (A4), and make its **first paragraph a
   single line** (A5). utoipa derives `summary` from that paragraph and
   `description` from the rest, and the generated reference renders the `summary`
   as a heading — a paragraph wrapped over two lines puts a newline inside a
   markdown heading and splits it. Everything the operation needs to say belongs
   in the doc comment, which is the only place the gate reads it from.
4. Run `just openapi-gen` and `just docs-openapi` to regenerate the spec
   artifact and the reference.
5. Run `just openapi-check`. Commit the handler, its annotation, and the
   regenerated spec together.

CI enforces steps 4 and 5: `just check` runs the source phase over the
annotations, diffs the spec artifact against a fresh generation, and fails on a
route the document does not carry, a documented operation no build mounts, or a
duplicate `operationId`.

### Removing a route

Delete the handler and its entry from `routes![]`. Run `just openapi-gen` and
`just docs-openapi`. The route disappears from the spec automatically, and
`--check-openapi` fails until the committed document is regenerated.

### Changing a route's signature

Change the route attribute. The path, verb, parameters and request body follow
it into the document without an annotation edit, so `just openapi-gen` and
`just docs-openapi` are the whole job. Edit the `#[utoipa::path(...)]`
annotation when the change is in what only it can say: responses, tags,
summaries, or a parameter description or schema the signature cannot carry.
`just openapi-check` fails in both cases until the committed spec is
regenerated, so the change cannot be merged undocumented.

## Key Design Decisions

### Why generate `paths(...)` at all?

`#[derive(OpenApi)]` references `__path_*` items generated by
`#[utoipa::path(...)]` proc-macros in the same crate. Cross-crate access would
require re-exporting every `__path_*` symbol from `backend`'s public API — more
boilerplate, not less. `#[utoipauto]` keeps that in one attribute on one struct
inside the crate, so `backend/src/openapi.rs` is an ordinary hand-written source
file: 22 lines, committed, and `#[derive(OpenApi)]` reads the same either way.
The serialized spec is committed for a different reason: it is the reviewed
artifact the CI gate diffs against.

### Why a proc macro rather than a `syn` scanner in `build.rs`?

The retired scanner was five source files plus three test files whose whole job
was to turn a directory of handlers into a list. `#[utoipauto]` is that list,
already written and tested upstream, and it removes a moving part rather than
adding one: no module list to forget an entry in, and no generated file a fresh
clone has to build before it can compile. What is given up is stated in
[the coverage section](#coverage-check).

### Why is the route-set check on the binary rather than an analyzer?

A route table assembled under a `cfg` feature exists only in a real build, so the
question "what does this product mount?" cannot be answered by reading source.
Reading the `routes![]` macros answers a different question — what the source
declares — and a declaration can disagree with what Rocket actually mounts. The
binary builds the real `build_rocket()`, so the check sees the same table the
server serves, from the same artifact a release ships.

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

### Why does the backend still run parity tests if the binary has the gate?

Because the gate is the expensive one. `openapi-routes-match` compiles a release-shaped
build with the frontend embedded; the tests run in `cargo test` against a test
build and are exercised with deliberately drifted inputs, which is how a
comparison that has stopped comparing is detected at all. The tests are the
self-checks; the flag is the gate. The asymmetry of the rule means the test build
cannot be the gate — it serves a different table than the shipped one.

## Testing the gate

Each comparison is a pure function exercised with drifted inputs, so a refactor
that empties a check fails a named test instead of quietly passing:

- `openapi_parity.rs` drives the asymmetric rule over fixtures in both
  directions, plus the `x-picasu-feature` pins.
- `openapi_contract.rs` drives the mounted-route comparison, the `operationId`
  uniqueness check, the exclusion list, and the shared `401` response component
  over both the real document and deliberately mutated copies of it.
- `backend/tests/probe_registration.rs` observes the `cfg(test)`-gated probe
  registration from outside `cfg(test)`.
- [`utils/openapi-sanity`](../../utils/openapi-sanity/README.md) drives every
  source rule over a fixture that must produce its finding, a conforming
  counterpart that must produce none, and the real router tree, which has to stay
  silent. Each fixture is pulled in with `include_str!`, so a deleted fixture
  breaks the build instead of skipping its test.

The markdown reference is generated but not drift-checked: `widdershins` is
fetched with `npx --yes` at generation time, which needs network access that CI
gates should not depend on. Regenerate it with `just docs-openapi` when the spec
changes; it therefore lags `backend/openapi.json` until someone does.

### Coverage check

The retired scanner cross-referenced `routes![]` against `#[utoipa::path]` and
printed a `cargo:warning=` per unannotated handler, plus an annotation-coverage
percentage. `#[utoipauto]` only sees annotations, so both diagnostics are gone —
and with them the check they provided: **a `routes![]` entry whose handler has no
`#[utoipa::path]` is now silent at build time.** It still fails
`--check-openapi`, because the route is mounted and the document omits it, but
only once someone runs the gate rather than on every build.

The half that is not lost is the one nothing else covered: `#[utoipauto]` cannot
silently skip a module, because there is no list to leave an entry out of. The
inverse direction — an annotation with no `routes![]` entry — is likewise
uncaught. Both were diagnostics, never failures.

## Files

| File                                                                   | Generator           | Role                                                                       |
| ---------------------------------------------------------------------- | ------------------- | -------------------------------------------------------------------------- |
| [`backend/src/openapi.rs`](../backend/src/openapi.rs)                  | `#[utoipauto]`      | The `#[utoipauto(paths = ...)]` configuration and the `ApiDoc` struct      |
| `backend/openapi.json`                                                 | `ApiDoc::openapi()` | Public OpenAPI 3.1 spec (committed, drift-checked)                         |
| `docs/openapi-reference.md`                                            | widdershins         | Human-readable API reference                                               |
| [`backend/src/openapi_public.rs`](../../backend/src/openapi_public.rs) | —                   | Public-spec filter and the backend-owned exclusion policy                  |
| [`backend/src/openapi_parity.rs`](../../backend/src/openapi_parity.rs) | —                   | The `--check-openapi` route-set gate and its asymmetric rule               |
| [`backend/src/spec_path.rs`](../../backend/src/spec_path.rs)           | —                   | The Rocket-to-`OpenAPI` path translation, in one place                     |
| [`backend/src/main.rs`](../../backend/src/main.rs)                     | —                   | `--dump-openapi` and `--check-openapi`                                     |
| `backend/src/tests/openapi_contract.rs`                                | —                   | Mounted-route / spec parity self-checks                                    |
| `backend/src/tests/openapi_parity.rs`                                  | —                   | The asymmetric rule over fixtures, and the feature-marker pins             |
| `backend/tests/probe_registration.rs`                                  | —                   | The `/get/test/` probe registration gate, seen from outside `cfg(test)`    |
| `backend/build.rs`                                                     | —                   | Writes the YAML scenario tests into `OUT_DIR`                              |
| [`utils/openapi-sanity`](../../utils/openapi-sanity/README.md)         | —                   | The `openapi-sanity` source gate: annotation shape and parameter agreement |
