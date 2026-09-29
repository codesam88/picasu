# OpenAPI Generator Pipeline

## The invariant

The backend exposes 61 documented operations across GET, POST, PUT, and DELETE
modules, plus the test-only probes and the static file server. The property this
pipeline exists to guarantee is one line:

> **The generated OpenAPI content must match what the backend implements at
> runtime.**

Everything below is machinery for enforcing that one property. It has two halves
with different ground truth, and the split is what makes the enforcement honest:

- **The route set** — which `(method, path)` pairs exist. Rocket's real mount
  table is the authority, so this half is _provable_ against a real build.
- **Operation detail** — the parameters, bodies, responses, tags and auth on each
  operation. Runtime knows nothing about these; they live only in the
  `#[utoipa::path]` annotations, so this half is only ever _statically derivable_
  against the handler source.

The checks are the means of enforcing the invariant, not the product. Two
independent derivations carry the weight, and their agreement is the evidence:

- **Static, by `syn`.** `scan_routes` and `scan_handlers` walk the source AST and
  find `routes![...]` registrations and `#[utoipa::path]` annotations. The
  `openapi-sanity` CLI runs these without compiling the backend, which is what lets
  it sit in the pre-commit hook and `just check`.
- **Runtime, from Rocket.** The real `build_rocket().routes()` reflects what the
  shipped binary actually mounts, including a `FileServer` mount that has no
  `routes![...]` to find. `--check-openapi` compares it against the committed
  spec.

Neither derivation takes its input from the other, and the CLI never takes a route
inventory from the server. That independence is deliberate — deriving one side
from the other would make them agree by construction and stop being evidence.

## Sources of truth

The code is the sole authority; the pipeline derives everything else from it:

- **`routes![]` macros** declare _which routes are registered_.
- **`#[utoipa::path]` annotations** declare _what each operation is_ — its
  parameters, bodies, responses, tag, `operationId`, and any feature marker.

From these, the pipeline generates the OpenAPI schema, a human-readable API
reference, and the coverage metrics. Generation reads a maintained file list
owned by `backend/build.rs`; the `openapi-sanity` CLI does not — it walks the
source tree itself, so the two are independent derivations whose agreement is
the evidence rather than two readers of one list. A `routes![]` block outside
the generation list never reaches the spec: the CLI's walk still reports the
operations the document lacks at analysis time, and `--check-openapi` fails the
route as mounted-but-undocumented if it ships anyway.

## Pipeline

```
┌─────────────────────┐     ┌──────────────────────────┐
│  routes![] macros   │     │  #[utoipa::path(...)]    │
│  (which routes)     │     │  (OpenAPI metadata)      │
└──────┬──────────────┘     └──────────┬───────────────┘
       │  scanned from the list         │  same list
       │  build.rs's list (syn)         │
       ▼                               ▼
┌──────────────────────────────────────────────────────┐
│   build.rs + utils/openapi-sanity (every build)      │
│                                                      │
│   1. Read its maintained file list                   │
│   2. Scan each routes![] block for handler names     │
│   3. Scan handler source for #[utoipa::path]         │
│   4. Warn on any missing annotations                 │
│   5. Write backend/src/openapi.rs                    │
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
│            the gate (just check / CI / release)      │
│                                                      │
│  Static, no build:   openapi-sanity check            │
│                      source vs. backend/openapi.json │
│  Artifact:           openapi-artifact                │
│                      regenerate and diff             │
│  Route set:          --check-openapi                 │
│                      build_rocket().routes() vs spec │
│                      (asymmetric, feature-aware)     │
└──────────────────────────────────────────────────────┘
```

### Steps

1. **`build.rs`** (automatic on every `cargo build`) — the build script:
   - Reads the API's source files from a maintained list it owns, which names
     only `backend/src/router` files — the `tests/` tree is not among them and
     no entry is `#[cfg(test)]`-gated. A `routes![]` block outside the list is
     caught twice: the CLI's walk reports the operations the document lacks at
     analysis time, and `--check-openapi` reports it as
     mounted-but-undocumented once it ships.
   - Parses the `routes![]` invocations in those files to discover every
     registered handler.
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
   filesystem — reading the files the list names, writing the generated files —
   and takes the `routes![]` entries and the per-function annotations from the
   shared crate. The build script is the list's only reader; the CLI walks the
   source tree itself, so what the generator produces and what the walk reads
   meet at the committed document, where disagreement is a finding rather than
   an impossibility.

2. **`just openapi-gen`** — runs the `picasu` binary with `--dump-openapi`
   (`cargo run -- --dump-openapi > backend/openapi.json`), which serves
   `openapi_public::public_json()`: the generated document minus the test-only
   probes, pretty-printed with sorted keys.

3. **`just docs-openapi`** — chains `openapi-gen` with:
   - `widdershins` to convert `openapi.json` → `docs/openapi-reference.md`
   - `prettier` for consistent markdown formatting

4. **`just openapi-check`** — the API contract gate, in three phases (see
   [The contract gate](#the-contract-gate)). It is part of `just check`, so it
   runs in CI, and the pre-commit hook runs it for any commit that touches
   `backend/`.

5. **`openapi_contract` tests** (`cargo test --lib openapi_contract`) — the
   document-shape checks and the parity _self-checks_: they drive the same
   `compare_route_set` `--check-openapi` uses, over the test build's route table
   and over deliberately drifted fixtures, so the comparison cannot be neutered
   and stay green. They also run the shared tag and auth policies over the
   generated public spec (the taxonomy below, and which operations can answer
   `401`). Both sides of the route comparison are normalized with `to_spec_path`,
   which lives in the backend alongside the exclusion policy. The route-set
   _verdict_ is `--check-openapi`'s, not theirs: a test build runs without
   `embed-frontend` and serves a different table than the one that ships.

6. **`openapi-sanity` tests** (`cargo test -p openapi-sanity`) — cover the
   scanner itself: `routes![]` entries in every layout, per-function annotation
   attribution, Rocket route attributes and their URIs, and the diagnostics for
   malformed input. A `routes![]` block in a file outside the generation list is
   the CLI walk's finding at analysis time rather than a test's.

7. **`openapi_parity` tests** (`cargo test --lib openapi_parity`) — the
   asymmetric parity rule over fixtures, since the feature-excuse branch has no
   instance in the repository: a mounted route the document omits fails, an
   ungated document-only operation fails, and a feature-gated operation absent
   from this build is excused — and is drift once the feature is on. The same
   file pins every `x-picasu-feature` value in the committed document to a
   feature `backend/Cargo.toml` declares.

## The contract gate

`just openapi-check` is the single command developers and CI run for the API
contract. It runs three phases, and a failure stops the recipe with a nonzero
exit, so the failing phase's own diagnostics are what the run shows:

1. **`openapi-sanity check`** — the static detail phase. Compares the annotated
   source with the committed `backend/openapi.json` without compiling the
   backend, so it runs in about a second and needs no build artifacts.
2. **`openapi-artifact`** — the generated-artifact phase. Regenerates the spec
   with `cargo run --package picasu -- --dump-openapi` into a temporary file and
   fails when it differs from the committed `backend/openapi.json`, printing the
   diff and the fix.
3. **`openapi-routes`** — the route-set phase, `picasu --check-openapi` run
   against a build configured like the shipped one. This is the only check that
   can prove the route-set half of the invariant, and it is pinned to the release
   feature set (`--features "embed-frontend auto-open-browser"`, the set
   `.github/workflows/release.yml` builds) so the build doing the checking is the
   build that ships: a feature-gated route only exists in the table of a build
   that has the feature, so checking a build without it could not see the route
   at all. The phase depends on the frontend bundle because `embed-frontend`
   embeds it.

The order matters only for the report: phase 1 is cheap and names the source that
has to change, so it runs before the phases that have to compile the backend.

### What the static phase checks

`openapi-sanity check` walks the source tree (`--source-root`, default
`backend/src`, skipping the `tests/` tree and `#[cfg(test)]` items) — no file
list, so its coverage is derived rather than asserted — and compares what it
finds with the committed document in both directions. It reports, one per line
as `file:line: message`:

| Finding                                                                                                | What it means                                                                       |
| ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| `registered in routes![] but the function carries no #[utoipa::path] annotation`                       | A mounted route with no operation of its own                                        |
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

Two inputs are skipped for the same reason — the source cannot name them, so
there is nothing to compare against the document and a finding would name a
placeholder rather than a change:

- a **request body whose type the analyzer cannot name**. The body type is read by
  unwrapping `Result`/`Option`/`Json`/`Form` down to a path, so a tuple, a slice or
  an array, a `dyn` trait object, a macro call, and a wrapper written without its
  type argument all leave the type unnamed. P3 compares nothing about that body —
  including the media type of a `Form` body, which is readable on its own but would
  need a message that names no type.
- a **query parameter the signature does not bind to a plain argument**. The
  `required` half of the parameter rules reads whether the argument Rocket binds to
  a `?<name>` is an `Option`, which needs an argument of that name. A guard declared
  under the parameter's name is not that argument, and a `?<name>` filled from a
  field of a `FromForm` struct bound to an argument of another name would mean
  reading the struct's definition, which the scan does not do.

No route in the repository is in either shape, and a test over `backend/src/router`
fails if one enters it, so a parameter the rules cannot read is a failing test rather
than an unchecked one.

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
backend. The mounted-route parity check has moved out of the test harness to
`--check-openapi`, which runs against a real build rather than a test build.

### The CLI

The crate behind the CLI, its library API and its limitations are documented in
[`utils/openapi-sanity/README.md`](../utils/openapi-sanity/README.md).

```
openapi-sanity check [options]
openapi-sanity help

--router-root <dir>     Router source root (default: backend/src/router)
--spec <file>           Committed OpenAPI document (default: backend/openapi.json)
--module <group>=<path> Router module whose routes![] block is part of the
                        contract, as a path relative to <router-root>.
                        Repeatable; replaces the built-in module list.
--exclude-prefix <path> Operation path prefix that is deliberately outside
                        the public contract. Repeatable.
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
in the source, and `/assets` is the static file mount rather than an API surface,
so `just openapi-check` passes both. They stay CLI arguments rather than constants
in the analyzer because it has no backend dependency: the list itself is declared
once, in `CONTRACT_EXCLUSION_PREFIXES` (`backend/src/openapi_public.rs`), and a
`openapi_contract` test reads the recipe and asserts its `--exclude-prefix` values
are exactly that list — a prefix added to either side alone fails the test, naming
what each side is missing. A caller that forgets an exclusion sees the omission as
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
backend test says what the annotations currently produce, and the artifact check
is what catches them disagreeing.

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
is what `--exclude-prefix` in the recipe passes to the CLI and what
`contract_exclusions_match_mounted_routes` holds to the routes it actually
matches, so a prefix cannot be added to one consumer alone.

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

### Where the parity tests went

`openapi_contract.rs` used to hold the two tests that decided this
(`every_mounted_route_is_documented`, `every_spec_operation_is_mounted`). They
were the wrong place for a load-bearing check: a test build runs without
`embed-frontend` and carries the `#[cfg(test)]` probe registrations, so it mounts
a different table than the one that ships, and `just test` only runs it when
someone runs `cargo test`. They are self-checks now — they drive
`compare_route_set` over the test build's table and over drifted fixtures, with
failure text that points at `--check-openapi` as the check that decides it. The
route-set verdict lives with the release gate, CI and the pre-commit hook, where
it runs against a build configured like the shipped one.

### Feature-dependent routes

A single canonical `openapi.json` describes every route _any_ build can expose —
the union across features, not one build's slice. An operation that exists only
under a feature carries that feature as a vendor extension, set with utoipa's
`extensions(...)` on `#[utoipa::path]`:

```rust
#[utoipa::path(
    get,
    path = "/get/index/experimental",
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

## Considered and rejected

Recorded so the next reader does not re-litigate them.

- **A runtime route inventory as the CLI's input.** Feeding `openapi-sanity` a
  table dumped by the server would cost the checker its no-compiled-artifact
  property — the reason it can sit in the pre-commit hook — and would make the
  external tool the authority on what the backend registers. The `syn` static scan
  stays the CLI's method.
- **Promoting parity to a launch-time check** (fail boot if the spec is missing or
  drifted). This couples server startup to a review artifact and gives every
  deployment a new way to fail to start. `--check-openapi` keeps the dependency in
  the repo and the release gate.
- **One spec per feature configuration.** It doubles the artifact count and the
  review surface, and turns "this route is missing" into drift between two files
  rather than between code and spec. A single canonical spec with feature markers
  keeps one source of record.
- **Deriving the scanned file set from the `mount()` calls in `builder.rs`.** It
  relocates the hardcoded list rather than removing it, and a `routes![]` block
  not yet wired into a mount would be invisible to the generator but visible to
  the runtime check. The maintained list plus `--check-openapi` backstops it
  instead: an unlisted route table that ships fails the runtime check as
  undocumented.
- **Walking the source tree to derive _generation's_ file set (retire the list
  from `build.rs`).** Generation is list-driven by decision, so a missed file
  cannot reach the spec and fails `--check-openapi` as mounted-but-undocumented
  when it ships — completeness is proven at runtime — and walking on every build
  adds file discovery to the hot path for a question the list answers directly.
  The CLI's file selection is the other half of the same question and does
  walk: its coverage is derived, and the two derivations meet at the committed
  document rather than at a shared input.

## Workflow

### Adding a new data API route

1. Add the handler function to a `routes![]` block. In a new file or group, add
   the file to the generation list in `backend/build.rs` so the generator emits
   it; the CLI's walk finds it either way and reports the operations the
   document lacks, and `--check-openapi` fails once the route is mounted.
2. Add `#[utoipa::path(...)]` with the route's HTTP method, path, parameters,
   and response types. The annotated `path` and verb must match the mounted
   route exactly; the gate fails on a mismatch. Set `tag = "..."` to the subject
   from the Tag conventions table — every operation must carry one, `pages` only
   on the SPA page routes, and the vocabulary is closed, so a new subject is a
   reviewed change to `KNOWN_TAGS` and this table. If the route is
   feature-gated, add `extensions(("x-picasu-feature" = json!("<feature>")))` so
   `--check-openapi` knows a spec operation may legitimately be absent from a
   build without it.
3. Add the operation to `AUTH_POLICY` in `utils/openapi-sanity/src/auth.rs`,
   naming the guards its parameters declare or marking it public. The gate fails
   on an operation in no policy entry, so this is part of adding the route.
4. Run `just openapi-gen` and `just docs-openapi` to regenerate the spec
   artifact and the reference.
5. Run `cargo test --lib openapi_contract` and `just openapi-check`.
6. Commit the handler, its annotation, its policy entry, and the regenerated spec
   together.

CI enforces steps 4 and 5: `just check` compares the source with the committed
spec, diffs the spec artifact, and `--check-openapi` fails on undocumented or
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

### Why is there a source-level check as well as a runtime route check?

They answer different questions, and neither replaces the other. `--check-openapi`
asks what Rocket _mounts_, which is the only view that can see a route table
assembled under a `cfg` feature or a `FileServer` mount with no `routes![]`; it
runs in a real build, because a test build without `embed-frontend` has a
different route table than the one that ships. The `openapi-sanity` CLI asks
whether the operations the source declares and the operations the committed
document carries are the same set, and whether each annotation's verb agrees
with its route attribute — questions about annotations and the artifact that
need no build, so they are cheap enough to sit in front of the artifact diff.
The path half of the route-attribute comparison was dropped: a path
disagreement always surfaces as mounted-but-undocumented at `--check-openapi`,
which is the check that can prove it.

The split is deliberate: anything that needs to know what Rocket mounts stays in
a real backend build, and the CLI stays a source analyzer. The route-set
comparison is redundant between the CLI and `--check-openapi` and resolves in
favour of the latter, which runs against the real mount.

### Why is the exclusion policy owned by the backend rather than the analyzer?

The exclusion list is a property of the _backend_, not of the analyzer: it says
which surfaces the public document is allowed to omit. The analyzer has no way to
know that, so the policy lives in one backend-owned place — a const in
`openapi_public` — and the generator, the CLI invocation and `--check-openapi` all
read the same definition. The reasons it cannot stay CLI-only:

- The prefix already exists in `openapi_public::TEST_ONLY_PATH_PREFIX`, and
  today the string is repeated in the `justfile` recipe and in each test that
  calls it. A second test-only prefix added to only some of them would either be
  stripped from the artifact and still gated, or gated and published; a test that
  reads the recipe and asserts it matches the Rust constant is what catches the
  first case.
- `--check-openapi` needs the same answer, and it does not go through the
  `justfile`. A policy the runtime check cannot read is a policy the runtime check
  will get wrong.

Exclusions stay minimal by construction. Feature-gated APIs are not exclusions —
the asymmetric parity rule handles them, because a build without the feature
legitimately does not mount the operation. Exclusions are for surfaces that are
not API in any build: test-only probes and static file mounts.

## Files

| File                                                                                   | Generator           | Role                                                                                    |
| -------------------------------------------------------------------------------------- | ------------------- | --------------------------------------------------------------------------------------- |
| [`utils/openapi-sanity/`](../utils/openapi-sanity/README.md)                           | —                   | `syn`-based route/annotation/guard scanner; source/spec, tag, auth and parameter checks |
| `utils/openapi-sanity/src/auth.rs`                                                     | —                   | The auth policy table and its checks                                                    |
| `utils/openapi-sanity/src/tags.rs`                                                     | —                   | The subject taxonomy and its checks                                                     |
| `utils/openapi-sanity/src/main.rs`                                                     | —                   | `openapi-sanity check` CLI (static check of the gate)                                   |
| `backend/src/openapi.rs`                                                               | `build.rs`          | ApiDoc struct with all routes (gitignored)                                              |
| `backend/openapi.json`                                                                 | `ApiDoc::openapi()` | Public OpenAPI 3.1 spec (committed, drift-checked)                                      |
| `docs/openapi-reference.md`                                                            | widdershins         | Human-readable API reference                                                            |
| [`backend/src/openapi_public.rs`](../../backend/src/openapi_public.rs)                 | —                   | Public-spec filter and the backend-owned exclusion policy                               |
| [`backend/src/openapi_parity.rs`](../../backend/src/openapi_parity.rs)                 | —                   | The `--check-openapi` route-set gate and its asymmetric rule                            |
| [`backend/src/main.rs`](../../backend/src/main.rs)                                     | —                   | `--dump-openapi` and `--check-openapi`                                                  |
| [`backend/src/tests/openapi_contract.rs`](../../backend/src/tests/openapi_contract.rs) | —                   | Document-shape assertions and the parity self-checks                                    |
| [`backend/src/tests/openapi_parity.rs`](../../backend/src/tests/openapi_parity.rs)     | —                   | The asymmetric rule over fixtures, and the feature-marker pins                          |
| [`backend/build.rs`](../../backend/build.rs)                                           | —                   | Scans its maintained file list, writes `openapi.rs`                                     |
