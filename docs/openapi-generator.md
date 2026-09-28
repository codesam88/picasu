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
duplicate `operationId`, a tag-taxonomy violation (an operation with no tag, a
tag outside the known set, or `pages` on a data-API operation), or a disagreement
between the auth policy and the documented `401` responses. The `openapi-sanity`
CLI compares the annotated source with the committed document without compiling
anything, and fails on the failures the runtime table cannot show: an annotation
whose path or verb disagrees with the route attribute it sits on, a handler
registered twice, a committed document that no longer matches what the source
declares, an operation grouped under a subject outside the taxonomy, or a handler
whose request guards do not match what the operation is supposed to require.

The tag taxonomy and the auth policy are owned by `openapi-sanity` and enforced by
the CLI; the backend tests run the same rules over the generated public spec, so
`cargo test` fails on the same drift without the CLI. See [Tag
conventions](#tag-conventions) and [Authentication
policy](#authentication-policy).

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
   the mounted Rocket routes with the public spec, and run the shared tag and auth
   policies over it (the taxonomy below, and which operations can answer `401`).
   Both sides of the route comparison are normalized with
   `openapi_sanity::to_spec_path`, the single path translation the crate owns.

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

| Finding                                                                                                | What it means                                                                       |
| ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| `registered in routes![] but the function carries no #[utoipa::path] annotation`                       | A mounted route with no operation of its own                                        |
| `the route serves X but its #[utoipa::path] declares Y`                                                | The route URI and the annotated path disagree after normalization                   |
| `the route declares GET but its #[utoipa::path] declares POST`                                         | The route attribute and the annotated verb disagree                                 |
| `GET X is declared in source but absent from the spec`                                                 | A handler the generator would register that the document does not carry             |
| `GET X is in the spec but no scanned route declares it`                                                | A committed operation no scanned source backs                                       |
| `registered in routes![] more than once (first at file:line)`                                          | A handler identity registered twice                                                 |
| `duplicate operationId \`id\` claimed by A, B`                                                         | Two operations share an id, which merges their generated client methods             |
| `METHOD PATH: declares no tags`                                                                        | An annotation that lost its `tag = "..."`                                           |
| `METHOD PATH: unknown tag \`x\``                                                                       | A subject nobody reviewed; the vocabulary is closed                                 |
| `METHOD PATH: data-API path carries the \`pages\` tag`                                                 | A data operation grouped with the SPA shell                                         |
| `METHOD PATH: SPA page path must carry \`pages\``                                                      | A page route grouped under a subject                                                |
| `the auth policy requires X but the handler declares no request guard`                                 | A guard removed from a protected handler, which is the change a consumer cannot see |
| `the auth policy requires X but the handler declares Y`                                                | A guard swapped for a weaker one, or a public route closed without a policy change  |
| `the deferred guard X is bound to \`auth\` and never enforced`                                         | A `GuardResult` the handler drops, so an unauthenticated caller is served           |
| `METHOD PATH is guarded but documents no 401 response`                                                 | A rejection the contract does not describe                                          |
| `METHOD PATH is a public operation but documents a 401`                                                | A public exception that has quietly started rejecting callers                       |
| `METHOD PATH is in no auth policy entry`                                                               | A new operation nobody classified as guarded or open                                |
| `auth policy entry \`id\` names an operation the document does not declare`                            | A removed operation whose policy entry was left behind                              |
| `the route binds path parameter X but the operation declares no in: path parameter…`                   | A `<segment>` the document never documents                                          |
| `the operation declares path parameter X but the route binds no such segment`                          | A documented placeholder the route does not serve                                   |
| `path parameter X … cannot be optional … required: false`                                              | A path parameter documented as optional                                             |
| `the documented path binds X but the route serves no such segment` (and the reverse)                   | A spec `{placeholder}` and a route `<segment>` that disagree                        |
| `the route binds query parameter X but the operation declares no in: query…` (and the reverse)         | An undocumented or over-documented `?<x>` binding                                   |
| `declares query parameter X as required: true but the handler binds it as an Option` (and the reverse) | A `required` flag that disagrees with `Option<T>`                                   |
| `the route binds its body to X but the operation declares no request body` (and the reverse)           | A `data = "<x>"` binding the operation does not document                            |
| `the operation declares request body X but the route binds its body to Y`                              | A schema naming a type the handler does not take                                    |
| `the operation describes the Form body as … — declare the body multipart/form-data`                    | A multipart form documented under another media type                                |
| `the operation declares no operationId…` / `declares operationId X but the handler is named Y`         | An operation a generated client could not call by its real name                     |
| `` `$ref` to the component schema X, which the document does not define ``                             | A reference to a schema that does not exist                                         |
| `component schema X is defined but nothing references it`                                              | An orphaned schema (`FileEntry` shipped as one)                                     |

Malformed input is reported rather than guessed at, on the same stream: an
unparsable `routes![]` entry, a route attribute without a string-literal URI, and
a syntax error all come from the analyzer the build script uses.

A handler whose annotation disagrees with its own route attribute is reported
once, locally, and is not also reported as document drift. The document is
generated from the annotation, so it inherits the disagreement; a second finding
would only restate the first.

## Authentication policy

An operation that documents a `401` and one that enforces a guard are
indistinguishable in the document. `GET /get/get-rows` and
`GET /get/get-scroll-bar` discarded their guard result with `let _ = auth;` and
answered 200 to an anonymous caller (`.plan/bug-get-rows-auth-guard-discarded.md`)
while the spec and the handler signature both looked correct. The gate therefore
reads the guards a handler's parameters declare and holds them to an explicit
policy.

### Observed guards

A parameter whose type is a known guard — matched on the last segment of its path,
so `GuardAuth`, `guards::GuardAuth` and `crate::router::auth::GuardAuth` are one
guard — is observed in one of two shapes:

| Shape                                                     | Read as        | Why                                                                          |
| --------------------------------------------------------- | -------------- | ---------------------------------------------------------------------------- |
| `auth: GuardAuth`, `auth: &GuardAuth`                     | direct guard   | Rocket runs it before the body; a rejected request never reaches the handler |
| `auth: GuardResult<GuardAuth>`, `auth: Option<GuardAuth>` | deferred guard | the outcome is handed to the body, so only the body can enforce it           |

A deferred binding is enforced when the body propagates it with `?`, returns it,
matches on it, or inspects it through a `Result`/`Option` method (`is_ok`,
`map_err`, `expect`, …). A wildcard binding — `let _ = auth;` — is a discarded
guard, and so is a binding the body never mentions. Both are reported against the
parameter's line, since that is where the fix goes.

The known-guard vocabulary is `openapi_sanity::KNOWN_GUARDS`, the single place a
guard type enters the analyzer. It is a hand-written claim about the codebase, and
`every_request_guard_in_the_backend_is_a_known_guard` reads the backend's
`FromRequest` implementations to hold it to that: a guard the analyzer does not
recognise is read as no guard at all, which the policy then reports as an
unguarded route rather than passing silently.

| Guard                    | Enforces                                                                         | Rejection |
| ------------------------ | -------------------------------------------------------------------------------- | --------- |
| `GuardAuth`              | the admin JWT cookie                                                             | 401       |
| `GuardShare`             | a share token from the headers or query, falling back to the admin cookie        | 401       |
| `GuardTimestamp`         | a bearer token whose `timestamp` claim equals the `timestamp` query parameter    | 401       |
| `TimestampGuardModified` | the same token, accepted while expired, for the endpoints that issue a fresh one | 401       |
| `GuardHash`              | a bearer token whose `hash` claim equals the serving id in the URL               | 401       |
| `GuardHashOriginal`      | the same check against the token's `asset_id` claim                              | 401       |
| `GuardUpload`            | a share allowed to upload, falling back to the admin cookie                      | 401       |
| `GuardReadOnlyMode`      | the server's read-only flag                                                      | 405       |

`GuardReadOnlyMode` is the one that answers 405, which is why the policy names
guard classes rather than counting guards: a read-only route is a write route that
is additionally closed while the server is read-only, and says nothing about
authentication.

### The policy table

`openapi_sanity::AUTH_POLICY` has one entry per documented operation, in
`utils/openapi-sanity/src/auth.rs`, and an operation with no entry is a finding:

```rust
AuthRule::guarded("get_data", &[GuardClass::Timestamp]),
AuthRule::guarded("compressed_file", &[GuardClass::Share, GuardClass::Hash]),
AuthRule::public("login"),
AuthRule { operation_id: "unauthorized", guards: &[],
           unauthenticated: Unauthenticated::LandingPage },
```

Three decisions are worth knowing when editing it:

- **Every operation is listed, not only the public ones.** Listing the exceptions
  and treating everything else as protected would leave the set of protected
  operations implicit, so deleting a guard from a protected handler would leave
  the policy untouched and the route open — the exact drift the policy exists to
  catch. The cost is one line per added operation.
- **Entries are keyed by `operationId`, not by `(method, path)`.** Authentication
  does not change when a route moves, and a path key would make every rename look
  like a new operation, with the old entry reported stale and the new one
  unlisted. A handler rename does change the id, and fails loudly as a stale entry.
- **A public entry states where a `401` comes from.** `Unauthenticated::Never` is
  the default; `CheckedByHandler` is the login endpoint, which compares a password
  and is how a caller obtains the token every other guard checks; `LandingPage` is
  `GET /unauthorized`, whose own response body is a 401.

Security is not inferred from subject tags. A tag is a documentation grouping, and
`pages` on a route says nothing about whether the data behind it is public.

### What the backend still checks

`cargo test --lib openapi_contract` keeps the document half of this policy —
`the_auth_policy_and_the_documented_unauthorized_responses_agree` — so the backend
fails on the same drift without the CLI. It is weaker than the CLI check, because
it can only read the generated document and not the handler signatures; that is why
it shares `AUTH_POLICY` rather than keeping a second list. The two document-shaped
checks the analyzer does not cover — the `Unauthorized` component is registered,
and every 401 is a `$ref` to it rather than an inlined literal — stay in the
backend. The mounted-route parity tests stay there too, for the reason above.

### The CLI

The crate behind the CLI, its library API and its limitations are documented in
[`utils/openapi-sanity/README.md`](../utils/openapi-sanity/README.md).

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

Every run ends in one of three markers: `openapi-sanity: PASS - ...` on stdout
for a clean run (exit `0`), `openapi-sanity: FAIL - N contract findings` as the
last line of stderr with the findings above it (exit `1`), or
`openapi-sanity: ERROR - <why>` for unusable input (exit `2`), which covers a
missing module, an unreadable or non-JSON document, a document with no `paths`
object, or a usage error.

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
- `tests/auth.rs` does the same for the auth policy: `unauthored/` carries one
  instance of every auth failure mode, and the whole report is asserted, so a rule
  that stopped reporting, or started reporting twice, is a test failure. It also
  runs the policy over the real router and requires it to be clean today, and
  checks that `AUTH_POLICY` has exactly one entry per documented operation.
- `tests/tags.rs` does the same for the taxonomy: `untagged/` carries one instance
  of every tag failure mode, its report is asserted, and the committed document is
  required to follow the vocabulary — including that the subjects it uses and the
  subjects the vocabulary names are the same set, so neither side can drift alone.
- `tests/params.rs` does the same for the parameter rules: each rule is driven
  over inline conforming and drifted trees and asserted as an exact diagnostic —
  file, line and message — and it runs the rules over the real router and
  document and requires them to be clean, so the repository's own parameters are
  part of what the gate holds.
- `tests/guards.rs` covers the guard observation itself — direct and qualified
  guard parameters, `Option<T>`, `GuardResult<T>` propagated, returned, matched
  and inspected, and the two discarded shapes — and holds `KNOWN_GUARDS` against
  the backend's actual `FromRequest` implementations.
- `tests/mutations.rs` starts from a conforming tree, breaks one thing, and
  requires the rule to appear: a guard removed from a protected handler, a
  `GuardResult` dropped with `let _ = auth;`, a public operation dropped from the
  policy, a tag removed from an operation, a tag outside the vocabulary, and
  `pages` on a data operation or missing from a page one. The mutations run over a
  copy in `target/`, and each restores to silence.
- `tests/cli.rs` covers what the library does not own: the argument handling, the
  exit codes, one finding per line on stderr, the four checks merged into one
  report with a shared finding printed once, and two runs printing the same
  report.
  It also runs the gate over the real `backend/src/router` and
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

The vocabulary is `openapi_sanity::KNOWN_TAGS` in
`utils/openapi-sanity/src/tags.rs` — a closed list, because a subject nobody
thought about is a reference group of one. Adding a subject is a deliberate change
to that line plus a row here; there is no per-operation exemption.

### The rules

`openapi_sanity::check_tags` holds the document to the vocabulary, in four rules:

| Finding                                 | Drift it catches                                    |
| --------------------------------------- | --------------------------------------------------- |
| the operation declares no tags          | an annotation that lost its `tag = "..."`           |
| unknown tag `x`                         | a subject added without a reviewed vocabulary entry |
| a data-API path carries the `pages` tag | a data operation grouped with the SPA shell         |
| an SPA page path must carry `pages`     | a page route grouped under a subject                |

`pages` is reserved in both directions. The expectation is derived from path shape
— the data-API prefixes `/delete/`, `/get/`, `/object/`, `/post/`, `/put/` and
`/upload` versus everything else — rather than from a hardcoded list of page paths,
because the document does not say which file annotated an operation; the assumption
that makes the derivation valid, and what would break it, is documented on
`is_data_api_path` in the crate. A data route added outside those shapes is
classified as a page and the placement rule fails loudly, which is the intended
outcome.

The rules are independent, so an untagged page path is reported twice: it is
missing a tag _and_ it is missing the reserved one. "Exactly one tag" is the
convention rather than a rule — no check enforces the upper bound — and
`the_repository_gives_every_operation_exactly_one_subject` holds the committed
artifact to it.

### Who runs them

`openapi-sanity check` runs the rules over the committed `backend/openapi.json`, so
`just check` and CI catch tag drift in the same run that catches a source/spec
disagreement. `cargo test --lib openapi_contract` runs the same rules over the
generated public spec, so `cargo test` fails on the same drift without the CLI. The
two read different documents on purpose: the CLI says what is committed, the
backend test says what the annotations currently produce, and phase 2 of the gate
is what catches them disagreeing.

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
   must carry one, `pages` only on the SPA page routes, and the vocabulary is
   closed, so a new subject is a reviewed change to `KNOWN_TAGS` and this table.
3. Add the operation to `AUTH_POLICY` in `utils/openapi-sanity/src/auth.rs`,
   naming the guards its parameters declare or marking it public. The gate fails
   on an operation in no policy entry, so this is part of adding the route.
4. Run `just openapi-gen` and `just docs-openapi` to regenerate the spec
   artifact and the reference.
5. Run `cargo test --lib openapi_contract` and `just openapi-check`.
6. Commit the handler, its annotation, its policy entry, and the regenerated spec
   together.

CI enforces steps 4 and 5: `just check` compares the source with the committed
spec, diffs the spec artifact, and the parity tests fail on undocumented or
stale operations.

### Removing a route

Delete the handler and its entry from `routes![]`, and its entry from
`AUTH_POLICY`. Run `just openapi-gen` and `just docs-openapi`. The route disappears
from the spec automatically, and the gate fails if the annotation or the policy
entry was left behind.

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

| File                                                         | Generator           | Role                                                                                     |
| ------------------------------------------------------------ | ------------------- | ---------------------------------------------------------------------------------------- |
| [`utils/openapi-sanity/`](../utils/openapi-sanity/README.md) | —                   | `syn`-based route/annotation/guard scanner, path rules, source/spec, tag and auth checks |
| `utils/openapi-sanity/src/auth.rs`                           | —                   | The auth policy table and its checks                                                     |
| `utils/openapi-sanity/src/tags.rs`                           | —                   | The subject taxonomy and its checks                                                      |
| `utils/openapi-sanity/src/main.rs`                           | —                   | `openapi-sanity check` CLI (phase 1 of the gate)                                         |
| `backend/src/openapi.rs`                                     | `build.rs`          | ApiDoc struct with all routes (gitignored)                                               |
| `backend/openapi.json`                                       | `ApiDoc::openapi()` | Public OpenAPI 3.1 spec (committed, drift-checked)                                       |
| `docs/openapi-reference.md`                                  | widdershins         | Human-readable API reference                                                             |
| `backend/src/tests/openapi_contract.rs`                      | —                   | Mounted-route / spec parity gate                                                         |
| `backend/build.rs`                                           | —                   | Reads the router files, writes `openapi.rs`                                              |
