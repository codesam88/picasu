# openapi-sanity

`openapi-sanity` checks source-level conventions in `#[utoipa::path]`
annotations and compares declared parameters and request bodies with the Rocket
route and handler signature beside them. It scans Rust source; it does not
validate generated OpenAPI documents or runtime behavior.

The checker implements **A1–A7, B1–B4 and P1–P4** from
[`.plan/openapi-annotation-checks.md`](../../.plan/openapi-annotation-checks.md),
which is the implementation record: it holds the per-rule rationale, the grammar
each rule can read and the limits it states. This file is the user-facing companion
— what each check enforces, what a finding looks like, and which test covers it, so
it is worth reading the test list to see what a claimed check actually catches. The
plan is authoritative where the two differ. What is not checked yet is under
[Known gaps](#known-gaps).

## Checks

### A1 — Do not restate the route path or bare verb

With `rocket_extras`, the route attribute supplies the operation path and HTTP
verb. The annotation must not repeat them.

```rust
// Finding: path and verb are already given by #[get].
#[utoipa::path(get, path = "/get/assets")]
#[get("/get/assets")]
```

**Tests:** `a_restated_path_or_verb_fails` (`a1_restated_route.rs`) covers path,
`get`, and `trace` restatements. `a_route_annotation_free_of_restatement_is_accepted`
(`a1_conforming.rs`) covers the valid form.

### A2 — Declare at least one response

Both a missing `responses(…)` and an empty `responses()` are findings. At least
one response entry is sufficient; this rule does not prescribe status codes.

```rust
#[utoipa::path(responses())] // Finding: no response is documented.
```

**Tests:** `a_missing_or_empty_responses_fails` (`a2_missing_responses.rs`) covers
both cases. `one_declared_response_is_enough` (`a2_conforming.rs`) is the valid
counterpart.

### A3 — Declare exactly one vocabulary tag

Each annotation must declare one `tag = "…"` from [`TAGS`](src/lib.rs), matching
the “Tag conventions” table in [`docs/openapi-generator.md`](../../docs/openapi-generator.md).
The ten tags are `albums`, `assets`, `auth`, `config`, `index`, `internal`,
`pages`, `serving`, `timeline`, and `upload`. Use `internal` for operations not
published in the API reference; there are no route-based exceptions.

**Tests:** `a_tag_outside_the_vocabulary_fails`
(`a3_tag_outside_the_vocabulary.rs`) covers no tag, an unknown tag, and multiple
tags. `every_tag_of_the_vocabulary_is_accepted` checks every entry in `TAGS`.

### A4 — Add a doc comment

Every annotated handler must have a doc comment. utoipa derives the operation's
`summary` and `description` from it.

**Tests:** `a_handler_without_a_doc_comment_fails`
(`a4_missing_doc_comment.rs`) and `a_doc_commented_handler_is_accepted`
(`a4_conforming.rs`).

### A5 — Give the first doc paragraph a non-empty, one-line summary

The first doc-comment paragraph becomes the operation summary. It must contain
text and occupy one line; later paragraphs may wrap. An empty doc comment is not
a summary.

**Tests:** `a_multi_line_summary_fails` (`a5_multi_line_summary.rs`),
`an_empty_summary_fails` (`a5_empty_summary.rs`), and
`a_one_line_summary_with_a_wrapped_description_is_accepted` (`a5_conforming.rs`).

### A6 — Do not set `operation_id` manually

utoipa derives the operation ID from the handler function name.

**Tests:** `a_hand_set_operation_id_fails` (`a6_hand_set_operation_id.rs`) and
`a_derived_operation_id_is_accepted` (`a6_conforming.rs`).

### A7 — Do not set operation-level `summary` or `description`

Both are derived from the doc comment. A per-response `description` is allowed;
it describes a response and is not derived from handler documentation.

**Tests:** `a_hand_set_summary_or_description_fails` (`a7_hand_set_prose.rs`) and
`a_derived_summary_and_description_are_accepted` (`a7_conforming.rs`).

### B1 — Bind every declared path/query parameter in the route

Each inline `params(…)` tuple in `Path` or `Query` must name a dynamic path or
query binding from the Rocket route attribute. For example, `<album_id>` binds
the path parameter `album_id`, and `?<limit>` binds the query parameter `limit`.
For a partial path segment, `<name..>` binds `name`.

Header and cookie parameters are not checked because route attributes do not
name them. An annotated function without a route attribute is left to
`openapi-routes-match`.

**Tests:** `a_parameter_the_route_does_not_bind_fails`
(`b1_parameter_the_route_does_not_bind.rs`) covers missing path and query names.
`parameters_the_route_binds_are_accepted` (`b1_conforming.rs`) covers matching
path/query names and partial segments.

### B2 — Match parameter optionality

utoipa 5.5 does not accept a `required` key in a parameter tuple. It derives the
published `required` value from the declared type: `Option<T>` is optional; other
types are required. The declared type's optionality must match the corresponding
handler argument. Both `Option<T>` and `std::option::Option<T>` are recognized.

**Tests:** `a_declared_optionality_the_argument_disagrees_with_fails`
(`b2_optionality_the_argument_disagrees_with.rs`) covers both mismatch
directions. `a_declared_optionality_the_argument_agrees_with_is_accepted`
(`b2_conforming.rs`) and `qualified_option_types_have_matching_optionality`
(`b2_qualified_optionality_agrees.rs`) cover matching types.

### B3 — Match a declared body schema to the route's body type

For a route binding `Json<T>` or `Data<T>`, a named `request_body` schema must
match `T`. Schema names are compared by their final path segment, matching
utoipa's component names.

`Value`, `serde_json::Value`, and `::serde_json::Value` declare an unconstrained
body and are not compared. Other names—including custom names ending in `Value`—
are concrete schemas. `Form<…>` bodies are not schema-compared because their
`TempFile<'r>` payloads have no schema type the annotation can name; B4 checks the
media type. Request-body forms the parser cannot read are not verified and are
part of the grammar limitation below.

**Tests:** `a_body_the_route_does_not_parse_fails`
(`b3_body_the_route_does_not_parse.rs`) covers mismatched schemas.
`a_custom_type_ending_in_value_is_still_compared`
(`b3_value_suffix_is_not_unconstrained.rs`) covers concrete names ending in
`Value`. `a_body_the_route_parses_is_accepted` (`b3_conforming.rs`) covers
matching, qualified names, unconstrained `Value`, and `Form` limitations.

### B4 — Declare `multipart/form-data` for form routes

A route binding `Form<…>` must declare the `multipart/form-data` media type. A
missing request body or a different media type is a finding. Both utoipa
spellings for declaring the media type are accepted.

**Tests:** `a_form_body_without_multipart_fails`
(`b4_form_body_without_multipart.rs`) covers missing and incorrect media types.
`a_form_body_naming_multipart_is_accepted` (`b4_conforming.rs`) covers both
accepted spellings and a JSON route.

### P1 — The declared success statuses are the handler's

The success comes from the return type: a fallible return (a `Result` or one of
the tree's `type X = Result<…>` aliases) contributes its payload's success; a
`Status` return is every `Status::` constant in its body; a `Redirect` return is
every constructor in its body (`Redirect::to` is 303 See Other, `Redirect::found`
is 302); anything else is 200. A success the handler cannot produce is a
finding, and an unknown `Status::` constant or `Redirect::` constructor is an
`unreadable_status` finding.

**Tests:** `p1_a_success_status_the_handler_never_returns_fails`
(`p1_status_return_missing.rs`), `p1_a_redirect_declared_as_200_fails`
(`p1_redirect_missing.rs`), `p1_an_unreadable_success_status_fails`
(`p1_unreadable_status.rs`), and the conforming counterparts
(`p1_status_return_conforming.rs`).

### P2 — Guard outcome statuses are declared

Every `impl FromRequest for G` under the source root contributes the literal
`Status::` values of its `Outcome::Error(…)` and `Outcome::Forward(…)` arms; a
guard named in a handler's signature must have those codes declared. An arm
whose status is computed (`err.http_status()`) makes the guard _dynamic_: the
computed code requires nothing, its literal codes still do, and the tree test
pins the dynamic set (currently `GuardShare`) so a new one is a decision, not a
silent skip. A signature ident with no entry in the guard table is skipped
without a finding — see the limits below.

**Tests:** `p2_a_guard_status_missing_from_responses_fails`
(`p2_guard_missing.rs`), `p2_a_declared_guard_status_is_accepted`
(`p2_guard_conforming.rs`), `p2_a_dynamic_guard_requires_nothing`
(`p2_dynamic_guard_conforming.rs`).

### P3 — Body error kinds are declared

Every `ErrorKind::K` literal in the handler body is translated through the
app-error map (`--app-error-map`, default `backend/src/error.rs`: the enum
variants plus the `http_status` match) and its status must be declared. A kind
the map does not declare is an `unknown_error_kind` finding — a typo is
reported, not guessed at. Only body-local literals count; a status a helper can
raise is deliberately not required.

**Tests:** `p3_a_body_error_kind_missing_from_responses_fails`
(`p3_kind_missing.rs`), `p3_a_declared_body_error_kind_is_accepted`
(`p3_kind_conforming.rs`), `p3_an_error_kind_the_map_does_not_know_fails`
(`p3_unknown_kind.rs`).

### P4 — Every declared status is one the handler can answer

`declared \ universe` is a finding, where the universe is success ∪ guards ∪
body kinds ∪ (fallible ? every `http_status` code : ∅) ∪ ({400} when the route
binds a body or query, because Rocket can reject those before the handler
runs). The universe over-approximates on purpose: helper-raised codes stay
inside it, which is what keeps this direction free of false positives, while a
success the handler never returns, an error code on a bindingless infallible
route, and codes outside every set (418, 207, …) still flag.

**Tests:** `p4_an_impossible_declared_status_fails` (`p4_exotic_status.rs`),
`p4_an_undeclarable_status_on_a_bindingless_route_fails`
(`p4_impossible_route_400.rs`), and the conforming counterparts
(`p4_universe_conforming.rs`, `p4_route_400_conforming.rs`).

## Supported syntax and limits

- B1/B2 read inline parameter tuples such as `("name" = Type, Query, …)`. A
  struct-style `params(SomeQueryStruct)` entry is counted as unreadable, not
  silently skipped. The tree test pins the unreadable count at zero; supporting
  `IntoParams` structs requires resolving their fields and overrides.
- B1 checks only `Path` and `Query` locations. It cannot compare header or cookie
  parameters with Rocket route bindings.
- B3 compares JSON/data schema names by their final path segment. Distinct Rust
  types with the same final name cannot be distinguished by this check.
- A9 grammar coverage is not complete. The current parser does not analyze
  `method(GET)`, `tags([…])`, `context_path`, or every grouped/otherwise unreadable
  value, and skips them silently rather than reporting them — so those spellings
  walk past A1 and A3. Tracked as an open step in the plan.
- P1 treats any return shape other than `Redirect`, `Status` or a fallible
  payload as a 200 success. Rocket's non-200 responders are handled; a future
  exotic responder extends that match.
- P2 derives guard statuses from literal `Status::` values in `FromRequest`
  outcome arms. A guard with a computed status still requires its literal
  statuses; only the computed one requires nothing, and
  `the_router_tree_is_clean` pins the dynamic set so a new computed status is a
  decision rather than a silent widening.
- **P2 and P4 skip a signature guard they cannot resolve** — an ident absent from
  the guard table is `continue`d, not reported. A crate alias such as
  `GuardResult` is expected here, but so is a guard whose `FromRequest` impl moved
  outside `--source-root`. Nothing counts the skips. All eight impls currently sit
  in `backend/src/router/auth.rs`, inside the scanned tree; move one and P2 goes
  quiet with no finding. Tracked as an open step in the plan.
- P3 reads only `ErrorKind::` literals in the handler's own body. Codes raised
  inside helpers are not required — they stay covered by the P4 universe
  instead.
- Route coverage, generated-document consistency, document validity, and runtime
  behavior are owned by other tools or tests. This checker only analyzes source
  annotations and route attributes.

## Known gaps

The rule set is complete for the grammar it reads, and the gaps below are open
work rather than defects in what is built. Each is an item in
[`.plan/openapi-annotation-checks.md`](../../.plan/openapi-annotation-checks.md)
with its status:

- The three annotation spellings listed above pass A1 and A3 unchecked.
- An unresolved signature guard is skipped by P2 and P4 without a finding.
- Two calibration rows the plan states are not enforced by the tree test: the
  declared body types that matched their route binding, and the dynamic-guard
  count. Only the handler count, the declaration inventory and the dynamic-guard
  name set are pinned.
- No rule covers `security(...)` or `securitySchemes`. The committed document
  declares neither, so no operation states that it requires authentication.

## Run it

```sh
cargo run -p openapi-sanity                                  # backend/src/router
cargo run -p openapi-sanity -- --source-root path/to/tree   # select a source root
cargo run -p openapi-sanity -- --app-error-map path/to/file # select the ErrorKind map
cargo run -p openapi-sanity -- --expect-at-least 60         # require a scan floor
```

The default source root is `backend/src/router`, relative to the workspace root.
`--source-root` selects another tree. `--app-error-map` selects the file P3
reads the `ErrorKind` → `http_status` mapping from (default
`backend/src/error.rs`); an unreadable or unrecognizable map exits 2 rather than
guessing. `--expect-at-least` sets a minimum number of annotated handlers; the
checker fails below that floor rather than reporting a potentially partial scan
as clean. The gate sets the floor to 60 for a tree currently containing 63
handlers.

Findings are written as `file:line: handler: message`. For example:

```text
b1_parameter_the_route_does_not_bind.rs:17: prefetch: the annotation declares the query parameter "nope", but the route binds no such query parameter: its query part is "?<locate>"
```

Exit codes: `0` means no findings, `1` means findings or a scan below the floor,
and `2` means the input could not be read or parsed.

## Tests and checks

The rule fixtures are in `tests/fixtures/openapi_annotations/` and are loaded
with `include_str!`. Every implemented rule has a failing case; checks with a
valid counterpart also test that conforming source is accepted. The app-error
map for the fixture tests is `tests/fixtures/app_error_map.rs`, shaped like
`backend/src/error.rs`. The `the_router_tree_is_clean` test runs all checks
over `backend/src/router`, pins 63 handlers, the B1/B2/B3 inventory and the
dynamic-guard set, and requires zero findings.

Run the tool's tests with:

```sh
cargo test -p openapi-sanity
```

`just openapi-check` runs this checker, verifies `backend/openapi.json`, and
compares mounted routes with documented operations. `just utils-test` runs the
checker tests; backend changes run both recipes in pre-commit. CI runs `just check`
and `just test`.
