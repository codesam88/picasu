---
status: in-progress
type: feature
priority: high
area: backend
---

## Purpose and ownership

This plan specifies source-level checks for `#[utoipa::path]` annotations and
their relationship to the Rocket route attributes beside them. `openapi-sanity`
owns facts that exist only in Rust source. `openapi-json-match` checks the
generated artifact, and `openapi-routes-match` checks mounted-route parity.

Rules belong here when they read source and no existing compiler, generator,
artifact check, or runtime route check enforces them.

## Rule index

| ID  | Requirement                                                             | Status    | Owner / scope                      |
| --- | ----------------------------------------------------------------------- | --------- | ---------------------------------- |
| A1  | Do not restate the route path or bare HTTP verb in the annotation.      | enforced  | `openapi-sanity`                   |
| A2  | Declare at least one response.                                          | enforced  | `openapi-sanity`                   |
| A3  | Declare exactly one tag from the documented vocabulary.                 | enforced  | `openapi-sanity`                   |
| A4  | Add a doc comment to each annotated handler.                            | enforced  | `openapi-sanity`                   |
| A5  | Give the doc comment a non-empty, one-line first paragraph.             | enforced  | `openapi-sanity`                   |
| A6  | Do not set `operation_id` manually.                                     | enforced  | `openapi-sanity`                   |
| A7  | Do not set operation-level `summary` or `description` manually.         | enforced  | `openapi-sanity`                   |
| B1  | Each declared path/query parameter name must be bound by the route.     | enforced  | Inline `params(…)` tuples only     |
| B2  | Declared and handler parameter types must agree on optionality.         | enforced  | Inline `params(…)` tuples only     |
| B3  | A declared request-body schema must match the route's parsed body type. | enforced  | Named JSON/data body types         |
| B4  | Form routes must declare `multipart/form-data`.                         | enforced  | `openapi-sanity`                   |
| A9  | Analyze legal utoipa spellings or fail closed on unsupported syntax.    | specified | Annotation grammar coverage        |
| C2  | Identify generated route groups that are not mounted.                   | specified | Improve route-parity diagnostics   |
| R1  | Place `POST` routes under `/post/`, unless explicitly excepted.         | planned   | Route naming convention            |
| S1  | Use the `pages` tag only for handlers in `router/get/get_page.rs`.      | planned   | Tag/route-family convention        |
| P1  | Match documented success status to the handler's response behavior.     | spike     | Measure return-type analysis first |

## A — annotation shape

These checks reject annotation properties that do not appear elsewhere in the
generated document as inconsistencies: the document is generated from the
annotation itself.

| ID  | Requirement                                                | Details                                                                                                                                                         |
| --- | ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A1  | No `path = "…"` and no bare verb.                          | `rocket_extras` derives both from the route attribute. `trace` is included among recognized bare verbs.                                                         |
| A2  | `responses(…)` is present and non-empty.                   | Missing and empty response lists are distinct findings.                                                                                                         |
| A3  | Exactly one tag from `TAGS`.                               | The ten tags are documented in `docs/openapi-generator.md`. There are no route-based exemptions; use `internal` for operations excluded from the published API. |
| A4  | A doc comment is present.                                  | `summary` and `description` are derived from handler documentation.                                                                                             |
| A5  | The first doc-comment paragraph contains text on one line. | Later paragraphs may wrap. An empty first paragraph does not provide a summary.                                                                                 |
| A6  | No operation-level `operation_id`.                         | utoipa derives it from the function name.                                                                                                                       |
| A7  | No operation-level `summary` or `description`.             | Per-response `description` is allowed; it describes a response and is not derived from handler documentation.                                                   |

### A1 — route facts

With `rocket_extras`, the route attribute supplies the path and HTTP verb. The
annotation must not repeat either as `path = "…"` or as a bare method token.
Each repeated fact is reported at its source location. The bare-token set includes
`get`, `post`, `put`, `delete`, `head`, `options`, `patch`, and `trace`.

### A2 — responses

An absent `responses(…)` and an empty `responses()` both fail. The absent form is
anchored to the handler signature; the empty form is anchored to the annotation.
At least one response is sufficient; the rule does not prescribe status codes.

### A3 — tags

Every annotation has exactly one `tag = "…"`, selected from the ten entries in
`TAGS` and the matching table in `docs/openapi-generator.md`. No route-specific
exception exists. The `internal` tag identifies operations that do not appear in
the published API document.

### A4/A5 — handler documentation

A4 requires a doc comment. A5 requires text in its first paragraph and requires
that paragraph to occupy one source line; subsequent paragraphs may wrap. This
paragraph is utoipa's operation summary and is rendered as a heading in the
generated reference.

### A6/A7 — derived fields

`operation_id` is derived from the handler name, and operation-level `summary` and
`description` are derived from the doc comment. Manual overrides duplicate those
facts without a comparison point, so A6 and A7 reject them. A7 does not reject a
per-response `description`.

### A9 — accepted annotation grammar

The parser must cover legal utoipa spellings used by the enforced rules. The
current gaps include `method(GET)`, `tags([…])`, `context_path`, and grouped or
otherwise unreadable values. For each form, either analyze it or report an
explicit unsupported-syntax finding. Add a failing fixture and a conforming
counterpart for each supported spelling.

## B — declarations against the route

The route attribute and handler signature are the source of truth. B1–B4 check
whether declarations added by `#[utoipa::path]` agree with those source facts.

| ID  | Requirement                                                             | Details                                                                                                                                                                                                                   |
| --- | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| B1  | Every declared path/query parameter is bound by the route.              | Path names match `<name>` segments; query names match `?<name>` bindings. Rocket's `<name..>` binds `name`. Header and cookie parameters have no corresponding route-attribute binding and are outside this check.        |
| B2  | Declared optionality matches the handler argument.                      | utoipa 5.5 has no `required` key for a parameter tuple; it derives `required` from the declared type. Compare whether the declared and handler types are `Option<…>`, including qualified `std::option::Option<…>` paths. |
| B3  | A named request-body schema matches the route's `data = "…"` body type. | Compare the schema name to the route's `Json<T>`/`Data<T>` payload type. Schema names use the last path segment, as they do in the generated document.                                                                    |
| B4  | A form body declares `multipart/form-data`.                             | A missing body or a different media type is a finding. Do not duplicate utoipa's media-type guessing rules.                                                                                                               |

### B1 — route-bound parameters

For `Path`, the declared name must appear as a dynamic route path segment. For
`Query`, it must appear in the route's query part. Rocket's partial segment
`<name..>` binds the name `name`; the dots are syntax, not part of the parameter
name. B1 does not compare headers or cookies because Rocket route attributes do
not name those inputs. An annotation without a Rocket route attribute is left to
`openapi-routes-match`.

### B1/B2 parameter syntax

The checker reads inline tuples such as `("name" = Type, Query, …)`. It counts
struct-style entries such as `params(SomeQueryStruct)` as unreadable rather than
silently skipping them. `the_router_tree_is_clean` pins the unreadable count at
zero; `only_the_inline_parameter_form_is_read` tests the parser. If a struct form
appears, add support for resolving its `IntoParams` fields or revise this limit
before accepting the change.

### B2 — optionality

utoipa 5.5 rejects `required = …` in a parameter tuple. It derives the published
`required` value from the declared type: `Option<T>` is optional; other types are
required. B2 compares that optionality with the handler argument's type, including
qualified `std::option::Option<T>` spellings. It reports both disagreement
directions.

### B3 — JSON/data body types

B3 unwraps `Option<…>` around the route's `Json<T>` or `Data<T>` binding and
compares the named request-body schema with `T`. It compares schema names by their
last path segment, matching utoipa's component-name behavior. `Value`,
`serde_json::Value`, and `::serde_json::Value` represent an unconstrained body;
concrete names ending in `Value` are still compared. A `Form<…>` payload is not
schema-compared because its `TempFile<'r>` fields have no schema type the
annotation can name; B4 checks its media type. Schema forms the parser cannot read
must be handled by A9 rather than silently treated as verified.

### B4 — form media type

A `Form<…>` route must explicitly declare `multipart/form-data` using either
accepted utoipa media-type syntax. Do not infer the expected media type by
reimplementing utoipa's type-based guessing. Both form routes in the current tree
declare the multipart media type.

## Current calibration

The router tree has 63 annotated handlers. The test suite pins these declaration
counts so a narrowed scan or newly unsupported parameter form fails:

| Fact                                           | Pinned value |
| ---------------------------------------------- | -----------: |
| Annotated handlers                             |           63 |
| Readable inline declared parameters            |            1 |
| Unreadable declared parameters                 |            0 |
| Declared request bodies                        |           24 |
| Declared body types matching the route binding |           21 |

All A1–A7 and B1–B4 checks pass over the router tree with zero findings. The 21
matching request bodies exclude the deliberately unconstrained `Value` body and
the two form bodies whose schema is not compared; B4 checks those two form routes'
media types.

## Planned work

### A9 — grammar coverage

Support or explicitly reject every legal utoipa spelling relevant to A1–A7 and
B1–B4. The current known gaps are `method(GET)`, `tags([…])`, `context_path`,
grouped values, and request-body forms the parser does not model. Tests must prove
that supported forms are analyzed and unsupported forms cannot silently pass.

### C2 — generated route groups

For every public `generate_*_routes()` group in `router/`, verify that the builder
mounts it. Route parity already detects a resulting documented-but-unmounted
operation; C2 should identify the missing generated group directly.

### R1 — POST route placement

Require `POST` routes to live under `/post/`. Any deliberate out-of-family route
must carry an explicit exception at the route declaration so the rule does not
need a separate copied route list. The current out-of-family route is
`POST /get/prefetch?<locate>`.

### S1 — page route tags

Require handlers in `router/get/get_page.rs` to use `pages`; handlers in other
router modules must not use it. Current calibration is 22 page handlers tagged
`pages` and zero other handlers using that tag.

### P1 — response status

Determine whether source analysis can reliably compare an operation's documented
success status with its handler response, including no-content responses. Measure
return-type analysis cost and false-positive risk before specifying enforcement.

### OpenAPI document linting

Add a separate document-linter phase for OpenAPI validity and document-only
conventions. Keep its configuration and rules distinct from source annotation
checks and mounted-route parity.

2026-10-04: baseline established with Spectral (`@stoplight/spectral-cli`,
`frontend/` devDependency, ruleset `.spectral.yaml` extending stock
`spectral:oas`). `just openapi-lint` is phase 3 of `just openapi-check`;
errors fail, warnings do not. Baseline is 0 errors, 53 warnings. Fixed at
baseline time: global `tags` + `info(description, contact)` in
`backend/src/openapi.rs` (61 `operation-tag-defined`, `info-contact`,
`info-description`), `oas3-api-servers` off (self-hosted, no canonical URL),
orphaned `FileEntry` stripped in `openapi_public.rs`
(`oas3-unused-component`). Remaining warnings are the backlog: 51
`operation-description` (description backfill), `path-params` on the
rank-disambiguated SPA fallbacks (warn), `operation-success-response` on the
intentional always-`401` `GET /unauthorized`.

The first concrete rule is parameter documentation: every published path or query
parameter needs a description, and a parameter with a closed value set should name
those values. `rocket_extras` derives the parameter list from the route attribute,
so satisfying this rule means declaring an inline parameter entry to carry the
description. Current calibration is 22 published parameters of which 21 have no
description; `auto_rename` on `POST /upload` is the only documented one. Decide
whether the rule belongs to the document linter or to a source rule before
implementing it, since the check and the fix are not in the same place.

## Execution and acceptance

`openapi-sanity` is the first phase of `just openapi-check`, scanning
`backend/src/router`. The recipe supplies an annotated-handler floor of 60
against the current count of 63. A short scan fails rather than reporting a
clean tree. Findings are reported as `file:line: message`; unreadable or
unparseable source also fails.

Keep parser and rule tests in `utils/openapi-sanity/tests/`. Each enforced rule
needs a failing case and a conforming case, and the router-tree test must pin the
coverage counts above and report zero findings. Backend changes run both
`just openapi-check` and `just utils-test` in pre-commit; CI runs `just check`
and `just test`.
