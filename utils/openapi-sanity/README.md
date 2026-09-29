# openapi-sanity

Static analysis of Rocket routes and `#[utoipa::path]` annotations, used as the
semantic phase of picasu's OpenAPI contract gate.

## What this exists for

The failure this catches is a spec that is internally consistent and still wrong.
`backend/openapi.json` is generated from the annotations, so a regeneration always
matches the source that produced it, and the build script only asks whether a
registered handler carries an annotation at all. Between those two facts there is a
class of drift nothing in the pipeline could see:

- an annotation whose verb disagrees with the route attribute it sits on — the route
  is mounted and served normally, the document describes something else. A path
  disagreement needs no rule of its own: the annotation's path absent from the
  document is the "declared in source but absent from the spec" finding below, and
  the route's own path absent from it is the backend's mounted-route comparison;
- a `#[utoipa::path]` on a sibling function in the same file being read as this
  function's own, which registers a route under another operation's metadata;
- a `GuardResult` the handler drops, so the operation documents a `401` and serves an
  unauthenticated caller `200` — the shape `GET /get/get-rows` and
  `GET /get/get-scroll-bar` had (`.plan/bug-get-rows-auth-guard-discarded.md`);
- a `routes![]` block the scanner does not read, leaving its routes mounted and
  undocumented with nothing in the build log;
- a `routes![]` entry split the wrong way: a one-line `routes![a, b]` read as one
  handler named `a, b`, which is how `POST /post/renew-timestamp-token` and
  `POST /post/renew-hash-token` went undocumented for as long as the single-line
  block in `router/auth.rs` existed.

The comparison runs on source, so it needs no compiled backend and runs before every
spec regeneration. What it cannot answer — whether Rocket really mounts the route
table in `routes![]` — belongs to the backend's mounted-route parity tests, which are
the only place feature-aware runtime inspection is possible.

## Usage

`just openapi-check` runs this crate. It is two phases in order, and a phase that
fails stops the recipe, so the failing phase's own diagnostics are what the run shows.
The semantic phase is:

```
openapi-sanity check \
    --router-root backend/src/router \
    --spec backend/openapi.json \
    --exclude-prefix /get/test/ \
    --exclude-prefix /assets
```

run from the repository root; the recipe passes absolute paths, and the first two flags
are the defaults, so `openapi-sanity check --exclude-prefix /get/test/ --exclude-prefix
/assets` is equivalent.
The second phase is `openapi-artifact`, which regenerates the document and diffs it
against the committed one. `just check` includes `openapi-check`, so CI runs it
(`.github/workflows/ci.yml`, the `just check` step), and `.githooks/pre-commit` runs it for
any commit that touches `backend/`. The pipeline as a whole is documented in
[docs/openapi-generator.md](../../docs/openapi-generator.md).

### Options

| Option                 | Meaning                                                                             |
| ---------------------- | ----------------------------------------------------------------------------------- |
| `--router-root <dir>`  | Router source root (default: `backend/src/router`)                                  |
| `--spec <file>`        | Committed OpenAPI document (default: `backend/openapi.json`)                        |
| `--module <g>=<path>`  | Router module whose `routes![]` block is part of the contract, relative to the root |
| `--exclude-prefix <p>` | Operation path prefix deliberately outside the public contract                      |

`--module` and `--exclude-prefix` are repeatable. `--module` **replaces** the built-in
module list, which is how the gate is pointed at something other than the repository.
`--exclude-prefix` stays a parameter because the analyzer has no backend dependency: it
describes the artifact, not the analysis — the public document strips the test-only
probe surface (`/get/test/`) while the handlers stay in the source, and `/assets`
serves the frontend's files rather than API operations. The list itself lives once,
in the backend's `CONTRACT_EXCLUSION_PREFIXES` (`backend/src/openapi_public.rs`), and a
test there reads the `justfile` recipe to hold its `--exclude-prefix` values to that
constant, so a caller that forgets an exclusion sees the omission as findings rather
than as a passing gate.

### Exit codes and the summary line

Every run reports one marker, in plain ASCII, so the verdict is readable in a log
without counting lines:

| Code | Meaning                                                                                                       | Marker                                                                                            |
| ---- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| 0    | Nothing to report                                                                                             | `openapi-sanity: PASS - 61 spec operations checked, no findings` (the whole of stdout)            |
| 1    | At least one contract finding, one per line on stderr                                                         | `openapi-sanity: FAIL - N contract findings` (last line of stderr)                                |
| 2    | Unusable input: a missing file, a document that is not JSON, a document with no `paths` object, a usage error | `openapi-sanity: ERROR - <why>` (stderr; the usage text follows when the arguments were at fault) |

A failing run prints its findings above the `FAIL` line as `file:line: message`, sorted
by file, line and message. Nothing reads the clock, the network, or the environment
beyond the working directory used to shorten labels, so two runs over one tree print
byte-identical output.

### As a library

`backend/build.rs` and `backend/src/tests/openapi_contract.rs` are the other consumers.
The library performs no I/O, prints nothing, and returns `Finding`s; what to do with one
is the caller's business.

```rust
use openapi_sanity::{SourceUnit, check_contract, scan_source, spec_operations};

let source = std::fs::read_to_string("backend/src/router/get/mod.rs")?;
let document: serde_json::Value =
    serde_json::from_str(&std::fs::read_to_string("backend/openapi.json")?)?;

// The parser: one file in, both views out.
let scan = scan_source("backend/src/router/get/mod.rs", &source, "get");
println!(
    "{} routes, {} handlers",
    scan.routes.len(),
    scan.handlers.len()
);

// The gate: source and document in, findings out. `units` has to hold the
// route tables *and* the files defining the handlers they register.
let units = vec![SourceUnit::for_relative_path(
    "backend/src/router/get/mod.rs",
    "get/mod.rs",
    &source,
)];
let spec = spec_operations(&document);
for finding in check_contract(&units, "backend/openapi.json", &spec, &["/get/test/", "/assets"]) {
    eprintln!("{finding}");
}
```

`referenced_handler_files` returns the further files a caller has to read to resolve the
handlers those route tables register, and `SCANNED_MODULES` is the list of route-table
files itself.

## What it checks

### The source/spec contract — `check_contract`

Six rules, each reported as a `file:line: message`:

| Finding                                                                          | Drift it catches                                         |
| -------------------------------------------------------------------------------- | -------------------------------------------------------- |
| `registered in routes![] but the function carries no #[utoipa::path] annotation` | A mounted route with no operation of its own             |
| `the route declares GET but its #[utoipa::path] declares POST`                   | Route attribute and annotated verb disagree              |
| `METHOD PATH is declared in source but absent from the spec`                     | A registered handler the document does not carry         |
| `METHOD PATH is in the spec but no scanned route declares it`                    | A committed operation no scanned source backs            |
| `registered in routes![] more than once (first at file:line)`                    | A handler identity registered twice                      |
| `duplicate operationId id claimed by A, B`                                       | Two operations share an id, merging their client methods |

A handler whose annotation disagrees with its own route is reported once, locally, and
is not also reported as document drift: the document inherits the disagreement from the
annotation, so a second finding would restate the first.

### Authentication — `check_auth`

Six rules, each reported the same way:

| Finding                                                                 | Drift it catches                                                            |
| ----------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `the auth policy requires X but the handler declares no request guard`  | A guard removed from a protected handler                                    |
| `the auth policy requires X but the handler declares Y`                 | A guard swapped for a weaker one, or a route closed without a policy change |
| `METHOD PATH is in no auth policy entry`                                | A new operation nobody classified                                           |
| `auth policy entry id names an operation the document does not declare` | A removed operation whose entry was left behind                             |
| `METHOD PATH is guarded but documents no 401 response`                  | A rejection the contract does not describe                                  |
| `METHOD PATH is a public operation but documents a 401`                 | A public exception that has started rejecting callers                       |

A guard is read from the handler's parameter list, matched on the last segment of its
type path: a bare or referenced `GuardX` is a **direct** guard Rocket runs before the
body, and a `GuardResult<X>`/`Option<X>` is **deferred** — only as strong as the body
makes it. A deferred binding is enforced when the body propagates it with `?`, returns
it, matches on it, or inspects it through a `Result` method (`is_ok`, `map_err`,
`expect`, …). A wildcard binding (`let _ = auth;`) and a binding the body never mentions
are both reported against the parameter's line, from the source scan, because that
defect holds for a route the policy does not describe.

`AUTH_POLICY` has one entry per documented operation — 61 today, keyed by
`operationId`. An operation with no entry is a finding, and so is an entry no operation
answers to; the set of protected operations is stated, not implied by a list of public
exceptions. `Unauthenticated` says where a guardless operation's `401` comes from:
`Never`, `CheckedByHandler` (the login endpoint), or `LandingPage`
(`GET /unauthorized`, whose own response body is a 401).

Security is not inferred from subject tags. A tag is a documentation grouping, and
`pages` on a route says nothing about whether the data behind it is public.

### The subject taxonomy — `check_tags`

Four rules, over the committed document only:

| Finding                                            | Drift it catches                                    |
| -------------------------------------------------- | --------------------------------------------------- |
| `METHOD PATH: declares no tags`                    | An annotation that lost its `tag = "..."`           |
| `METHOD PATH: unknown tag x`                       | A subject added without a reviewed vocabulary entry |
| `METHOD PATH: data-API path carries the pages tag` | A data operation grouped with the SPA shell         |
| `METHOD PATH: SPA page path must carry pages`      | A page route grouped under a subject                |

The rules are independent, so a page path with no tags is reported twice: it is missing
a tag _and_ missing the reserved one. Whether a path is a data-API operation is derived
from path shape, because the document does not say which file annotated an operation;
the assumption that makes the derivation valid is stated on `is_data_api_path`.

### Parameters and the request body — `check_params`

Five rule groups over each operation's inputs, each reported the same way:

| Finding                                                                                                                            | Drift it catches                                                |
| ---------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| `the route binds path parameter X but the operation declares no in: path parameter by that name`                                   | A `<segment>` the document never documents                      |
| `the operation declares path parameter X but the route binds no such segment`                                                      | A documented placeholder the route does not serve               |
| `path parameter X is bound by the route and cannot be optional, but the operation declares required: false`                        | A path parameter documented as optional                         |
| `the documented path binds X but the route serves no such segment` / `the route serves a X segment but the documented path binds…` | A spec `{placeholder}` and a route `<segment>` that disagree    |
| `the route binds query parameter X but the operation declares no in: query…` / the reverse                                         | An undocumented or over-documented `?<x>` binding               |
| `declares query parameter X as required: true but the handler binds it as an Option` (and the reverse)                             | A `required` flag that disagrees with `Option<T>`               |
| `the route binds its body to X but the operation declares no request body` / the reverse                                           | A `data = "<x>"` binding the operation does not document        |
| `the operation declares request body X but the route binds its body to Y`                                                          | A schema naming a type the handler does not take                |
| `the operation describes the Form body as … — declare the body multipart/form-data` (and `…name the schema X or declare…`)         | A multipart form documented under another media type            |
| `the operation declares no operationId…` / `declares operationId X but the handler is named Y`                                     | An operation a generated client could not call by its real name |
| `` `$ref` to the component schema X, which the document does not define ``                                                         | A reference to a schema that does not exist                     |
| `component schema X is defined but nothing references it`                                                                          | An orphaned schema (`FileEntry` shipped as one)                 |

The rules compare sets, names and flags, not schema types: a `timestamp` declared
as `string` would not be reported. Placeholders are compared as names — the route's
`<_path..>` and the document's `{path}` are one `path` once each side is read for
what it spells — so no whole-path translation is involved, and a query parameter's
`required` flag is checked against `Option<T>` on the bound argument. A `Form<T>` body
is satisfied by a `multipart/form-data` content type; naming the schema under any other
media type is a finding, because the media type is how a caller knows to send the
fields.
Two inputs are skipped where the source cannot name them at all — an unnamed body
type and a query parameter no plain argument binds — and both are listed under
[Limitations](#what-is-not-checked).

## How it is tested

`cargo test -p openapi-sanity` runs:

- **Unit tests per module** — `routes.rs` (every `routes![]` layout, the line each entry
  is reported on, malformed entries), `handlers.rs` (route attributes, per-function
  `#[utoipa::path]` attribution, nested token groups), `guards.rs` (direct vs deferred
  bindings, every enforcement shape, both discard shapes, and `KNOWN_GUARDS` held
  against the backend's actual `FromRequest` implementations). The
  Rocket-to-OpenAPI translation lives with the backend comparison that uses it, so
  its cases run under `cargo test --lib openapi_contract` instead.
- **Fixture trees** under `tests/fixtures/{clean,drift,unauthored,untagged}`. `clean`
  reports nothing; each of the other three carries one instance of every failure mode of
  its check, and the whole report is asserted as an exact list — file, line and message —
  so a rule that stopped reporting, or started reporting twice, fails.
- **Mutation tests** — `mutations.rs` starts from a conforming tree, breaks exactly one
  thing, requires the rule to appear, and restores it to require silence. A rule that
  fires for any reason at all fails there.
- **The repository is clean today** — `cli.rs`, `auth.rs`, `tags.rs` and `params.rs`
  each run the gate over the real `backend/src/router` and `backend/openapi.json`
  and require no findings, so the gate cannot be neutered and stay green on the
  repository.
- **Regression tests** — `regressions.rs` pins the _incidents_ rather than the rules:
  the source shape the repository actually shipped with, each asserted to produce its
  exact finding and, in its conforming form, silence. An incident already pinned
  elsewhere is cited there rather than duplicated; the doc comment on each test names
  what the other test does not reach.

## Limitations

### Picasu-specific by design

The analyzer is generic; the vocabulary it checks against is not. For a backend other
than picasu:

| Constant                                                  | What it holds                                                                                                                                         | How a consumer changes it                                            |
| --------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `SCANNED_MODULES` (`modules.rs`)                          | Picasu's router files that carry route tables                                                                                                         | Edit the constant — **not** a CLI input                              |
| `AUTH_POLICY` (`auth.rs`)                                 | All 61 picasu `operationId`s with their guard classes                                                                                                 | Edit the constant — **not** a CLI input                              |
| `KNOWN_GUARDS` (`guards.rs`)                              | Picasu's guard types, matched on the last path segment                                                                                                | Edit the constant and the `GuardClass` mapping — **not** a CLI input |
| `KNOWN_TAGS`, `DATA_API_PREFIXES`, `PAGE_TAG` (`tags.rs`) | Picasu's subject taxonomy, the path shapes that identify data routes, and the tag reserved for the SPA shell                                          | Edit the constants — **not** a CLI input                             |
| `--exclude-prefix /get/test/ --exclude-prefix /assets`    | Picasu's test-only probe surface and static file mount, passed by `just openapi-check`, held to the backend's `CONTRACT_EXCLUSION_PREFIXES` by a test | Pass your own — this one _is_ a CLI input                            |

The first four rows are compile-time constants, not configuration: a consumer with a
different backend edits the source. `--exclude-prefix` and `--module` are the only
vocabulary a consumer can supply at the command line, which is why the module list has a
CLI override but the auth policy and the taxonomy do not.

Two of them are hand-written claims about the codebase rather than facts derived from
it, and each has a test holding it to its claim rather than to a constant:
`KNOWN_GUARDS` is read against the backend's `FromRequest` implementations (a guard the
analyzer does not recognise is read as _no_ guard, so the policy reports the route as
unguarded rather than passing it silently), and the two lists `KNOWN_TAGS` and the
document's own subjects are asserted to be the same set.

### Source analysis, not runtime

The gate reads files. It cannot prove which routes Rocket actually mounts under a given
feature or `cfg` set, and it does not try: a route table assembled under a feature flag
reads as the table in the source. That question stays with the backend's mounted-route
parity tests in `backend/src/tests/openapi_contract.rs`.

### No `cfg` or feature resolution

The crate contains no reference to `cfg` at all. A route or handler behind
`#[cfg(feature = "...")]` is read as always present, and a `routes![]` block inside one
is scanned. A gated route therefore has to be documented and classified, or excluded by
prefix — the conservative direction, and the one that makes a stale gate visible when
the feature is removed.

### Syn-only

Source is parsed with `syn`; it is not compiled, type-checked or name-resolved, and
macros are not expanded beyond what the visitor reads as tokens. Consequences:

- a handler's identity is its group, the file it is declared in and its own function
  name; `use` statements are not resolved, so a handler re-exported or aliased under
  another name is not visible under that name;
- a `routes![]` entry is recognised as a macro whose last path segment is `routes`, and
  its entries must be plain `ident` or `path::ident` — anything else is reported rather
  than resolved;
- whole-path translation is not done here at all: `to_spec_path` lives in the backend
  (`backend/src/spec_path.rs`), beside the comparisons that use it, and the rules in
  this crate compare segment and placeholder _names_, which need no translation — so
  there is no second copy to drift from.

### What is not checked

- **A body whose type the analyzer cannot name.** The body type is read by unwrapping
  `Result`/`Option`/`Json`/`Form` down to a path, so a tuple, a slice or an array, a
  `dyn` trait object, a macro call and a wrapper written without its type argument
  leave the type unnamed — and P3 compares nothing about that body. A `Form` body of
  such a type is also not reported for a wrong media type, which is readable on its
  own: every finding P3 makes about a form body names the inner type, and reporting the
  media type alone would need a second message shape for a type that has no name. No
  route in the repository is in this shape, and a body is normally a struct.
- **A query parameter the signature does not bind to a plain argument.** P2's `required`
  half reads whether the argument Rocket binds to a `?<name>` is an `Option`, which
  needs an argument of that name. A guard declared under the parameter's name is not
  that argument, and a `?<name>` filled from a field of a `FromForm` struct bound to an
  argument of another name would mean reading the struct's definition, which the scan
  does not do. Both are reported as nothing rather than guessed at; a test over the
  repository fails if a route enters either shape, so the silence cannot spread.
- **Schema content and response types.** The parameter rules read `parameters`
  and `requestBody` for names, flags, media types and the schema a `$ref` names —
  not the schema bodies themselves. A parameter declared with the wrong `type`, a
  response schema that does not match the handler's return type, or a wrong field
  type inside `components` is invisible here.
- **`operationId` stability across releases.** Only collisions and disagreement
  with the handler name are reported. Renaming an operation is invisible; a stale
  `AUTH_POLICY` entry is what surfaces a handler rename, and it surfaces as a
  policy finding, not as a stability check.
- **General OpenAPI linting.** Nothing validates the document against the OpenAPI
  specification, and the two checks the backend keeps for that reason — the
  `Unauthorized` component being registered, and every 401 being a `$ref` to it rather
  than an inlined literal — stay in `backend/src/tests/openapi_contract.rs`.
- **Taint or reachability analysis.** Nothing follows a request into the operations it
  reaches. `GET /get/get-scroll-bar` once panicked on an unknown snapshot id on
  unauthenticated input; that is a reachability question, and the gate cannot answer it
  either way. Request-to-panic analysis remains a separate task by decision
  (non-goals in
  [`.plan/openapi-contract-hardening.md`](../../.plan/openapi-contract-hardening.md)).

### Assumptions about its input

- The document must be JSON with a `paths` **object**. The CLI rejects anything else
  with exit 2 before it reaches the checks, because a document that does not parse is
  unusable input rather than a contract with no operations.
  `spec_operations` panics on such a document if a library caller passes one; the CLI's
  `read_spec` is the guard the binary relies on.
- A path item may carry non-operation keys (`parameters`, `summary`, `servers`, `$ref`)
  and they are skipped. A path item that declares an operation under a key this crate
  does not recognise is read as a document with **fewer** operations than it has.
- A non-numeric response key (`default`, `4XX`) is not read as a status code.
- A `tags` value that is not an array of strings is read as carrying no tags, which
  `check_tags` then reports.
- Every `source` string handed to `scan_source`, `scan_routes` or `scan_handlers` must be
  a complete parseable file. A syntax error becomes a `Finding` with a file and a line
  rather than a partial scan, and a fragment is a syntax error.
