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
generated artifact, `openapi-lint` checks document-level conventions, and
`openapi-routes-match` checks mounted-route parity.

Rules belong here when they read source and no existing compiler, generator,
artifact check, or runtime route check enforces them. Everything below is the
implementation record and the authority for these rules: what each rule compares,
the grammar it can read, the limits it states, and the fixture that proves it.
`utils/openapi-sanity/README.md` is the user-facing companion — it restates each
check with its test, so a reader can see what a claimed check would actually
catch, and it stays subordinate to this file where the two disagree.

Two principles shaped the design. Neither is absolute: each is a preference that
gives way when the cost of not knowing a fact exceeds the cost of stating it.

**Prefer deriving a fact to stating it.** Where the repository already expresses a
fact in source, read it from there — guard statuses from the `FromRequest` impls,
`ErrorKind` codes from `--app-error-map`, paths, verbs and parameters from the
route attribute and handler signature. A restated copy is a second place to forget,
and a gate built on it reports the stale copy rather than the truth.

**When the repository states a fact nowhere machine-readable, state it here and make
the statement checkable.** Deriving is not always possible, and a rule that refuses
to name an unknowable requirement cannot enforce it. A stated fact earns its place
by meeting three conditions:

- it is a closed, small set — a vocabulary or a mapping, not a copy of logic;
- it carries a comment saying what it asserts, where that comes from, and what
  would invalidate it (a crate version, a documentation link);
- where the repository states the same fact elsewhere, a check keeps the two in
  agreement, so the duplication is a verified mirror instead of a drift risk.

The checker states two kinds of fact today, and neither yet meets the third
condition. [`TAGS`] mirrors a table in `docs/openapi-generator.md` with nothing
asserting they agree; the Rocket `Status` → code table and the verb token sets
record crate behaviour a dependency bump could change silently. Both are open work
below, alongside the third-party assumptions the rules rest on — that utoipa derives
`operation_id` from the function name and `required` from `Option`, and that
`Redirect::to` answers 303. Those assumptions are best turned into assertions over
the generated artifact, where a behaviour change fails a named test instead of
quietly invalidating a rule.

**A rule that cannot see its subject must say so.** Skipping silently turns a
narrowed check into a clean report. Every limit below is either enforced by a pin
or listed as open work.

## Rule index

| ID  | Requirement                                                             | Status   | Scope                                                   |
| --- | ----------------------------------------------------------------------- | -------- | ------------------------------------------------------- |
| A1  | Do not restate the route path or bare HTTP verb in the annotation.      | enforced | `openapi-sanity`                                        |
| A2  | Declare at least one response.                                          | enforced | `openapi-sanity`                                        |
| A3  | Declare exactly one tag from the documented vocabulary.                 | enforced | `openapi-sanity`                                        |
| A4  | Add a doc comment to each annotated handler.                            | enforced | `openapi-sanity`                                        |
| A5  | Give the doc comment a non-empty, one-line first paragraph.             | enforced | `openapi-sanity`                                        |
| A6  | Do not set `operation_id` manually.                                     | enforced | `openapi-sanity`                                        |
| A7  | Do not set operation-level `summary` or `description` manually.         | enforced | `openapi-sanity`                                        |
| B1  | Each declared path/query parameter name must be bound by the route.     | enforced | Inline `params(…)` tuples only                          |
| B2  | Declared and handler parameter types must agree on optionality.         | enforced | Inline `params(…)` tuples only                          |
| B3  | A declared request-body schema must match the route's parsed body type. | enforced | Named JSON/data body types                              |
| B4  | Form routes must declare `multipart/form-data`.                         | enforced | `openapi-sanity`                                        |
| P1  | Documented success status matches the handler's response behavior.      | enforced | Return-type and body-const analysis                     |
| P2  | Statuses a signature guard can answer are declared.                     | enforced | `FromRequest` impls in the scanned tree                 |
| P3  | Statuses from body `ErrorKind::` literals are declared.                 | enforced | `AppError::http_status` map from `backend/src/error.rs` |
| P4  | Every declared status is one the handler can answer.                    | enforced | Universe check, over-approximating                      |
| A9  | Analyze legal utoipa spellings or fail closed on unsupported syntax.    | open     | Annotation grammar coverage                             |

## A — annotation shape

These rules reject annotation properties that cannot appear elsewhere in the
generated document as inconsistencies, because the document is generated from
the annotation itself. Nothing downstream re-derives them from the route.

| ID  | Requirement                                                | Reading                                                                    |
| --- | ---------------------------------------------------------- | -------------------------------------------------------------------------- |
| A1  | No `path = "…"` and no bare verb.                          | `rocket_extras` derives both from the route attribute.                     |
| A2  | `responses(…)` is present and non-empty.                   | Missing and empty are distinct findings.                                   |
| A3  | Exactly one tag from `TAGS`.                               | No route-based exemptions; `internal` marks unpublished operations.        |
| A4  | A doc comment is present.                                  | `summary` and `description` are derived from it.                           |
| A5  | The first doc-comment paragraph contains text on one line. | Later paragraphs may wrap.                                                 |
| A6  | No operation-level `operation_id`.                         | utoipa derives it from the function name.                                  |
| A7  | No operation-level `summary` or `description`.             | Per-response `description` is allowed and is not derived from the handler. |

**A1 — route facts.** The route attribute supplies the path and HTTP verb; the
annotation must not repeat either as `path = "…"` or as a bare method token. Each
repeated fact is reported at its source location. The bare-token set is `get`,
`post`, `put`, `delete`, `head`, `options`, `patch`, `trace`.

**A2 — responses.** An absent `responses(…)` is anchored to the handler signature,
an empty `responses()` to the annotation. At least one response suffices; the rule
prescribes no status codes.

**A3 — tags.** Every annotation has exactly one `tag = "…"` from the ten entries in
`TAGS`, which is a copy of the table in `docs/openapi-generator.md`. No
route-specific exception exists.

**A4/A5 — handler documentation.** A4 requires a doc comment. A5 requires text in
its first paragraph and requires that paragraph to occupy one source line. That
paragraph is utoipa's operation summary and renders as a heading in the generated
reference.

**A6/A7 — derived fields.** `operation_id` comes from the handler name, and
operation-level `summary` and `description` come from the doc comment. Manual
overrides duplicate those facts without a comparison point. A7 does not reject a
per-response `description`, which describes one response instead of the operation.

**Fixtures.** `a1_restated_route.rs` / `a1_conforming.rs`;
`a2_missing_responses.rs` / `a2_conforming.rs`;
`a3_tag_outside_the_vocabulary.rs` / `a3_conforming.rs`;
`a4_missing_doc_comment.rs` / `a4_conforming.rs`;
`a5_empty_summary.rs`, `a5_multi_line_summary.rs` / `a5_conforming.rs`;
`a6_hand_set_operation_id.rs` / `a6_conforming.rs`;
`a7_hand_set_prose.rs` / `a7_conforming.rs`.

## B — declarations against the route

The route attribute and handler signature are the source of truth. B1–B4 check
whether declarations added by `#[utoipa::path]` agree with those source facts.

| ID  | Requirement                                                             | Reading                                                                                                    |
| --- | ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| B1  | Every declared path/query parameter is bound by the route.              | Path names match `<name>` segments, query names `?<name>` bindings. Headers and cookies are out of scope.  |
| B2  | Declared optionality matches the handler argument.                      | utoipa 5.5 has no `required` key; it derives it from the declared type. Both disagreement directions fail. |
| B3  | A named request-body schema matches the route's `data = "…"` body type. | Compared by last path segment, as utoipa publishes component names.                                        |
| B4  | A form body declares `multipart/form-data`.                             | A missing body or another media type is a finding. utoipa's guessing rules are not reimplemented.          |

**B1/B2 parameter syntax.** The checker reads inline tuples such as
`("name" = Type, Query, …)`. A struct-style `params(SomeQueryStruct)` entry is
counted as _unreadable_ rather than skipped, and the tree test pins that count at
zero, so the first `IntoParams` struct in the tree fails a test instead of
quietly narrowing B1 and B2. `only_the_inline_parameter_form_is_read` covers the
parser; supporting the struct form means resolving its fields and overrides.

**B2 — optionality.** utoipa 5.5 rejects `required = …` in a parameter tuple: it
derives the published value from the declared type, where `Option<T>` is optional
and anything else is required. B2 compares that against the handler argument's
type, including qualified `std::option::Option<T>`.

**B3 — JSON/data body types.** B3 unwraps `Option<…>` around the route's `Json<T>`
or `Data<T>` binding and compares the declared schema name with `T`. `Value`,
`serde_json::Value` and `::serde_json::Value` are an unconstrained body and are not
compared, while a concrete name ending in `Value` still is. The reverse direction
remains a finding: naming a concrete type on a route that binds `Json<Value>` is a
claim about what the route parses. A `Form<…>` payload is not schema-compared
because its `TempFile<'r>` fields have no nameable schema; B4 covers its media
type instead. Schema forms the parser cannot read belong to A9, not to "verified".

**Fixtures.** `b1_parameter_the_route_does_not_bind.rs` / `b1_conforming.rs`;
`b2_optionality_the_argument_disagrees_with.rs`,
`b2_qualified_optionality_agrees.rs` / `b2_conforming.rs`;
`b3_body_the_route_does_not_parse.rs`, `b3_value_suffix_is_not_unconstrained.rs` /
`b3_conforming.rs`; `b4_form_body_without_multipart.rs` / `b4_conforming.rs`.

## P — response statuses

The route attribute and handler are the source of truth for what an operation can
answer; `responses(…)` is the annotation's claim about it. Section P checks the
claim in both directions, from source only — the document is generated from the
annotation, so it cannot disagree with it and is the wrong place to look.

Two derived inputs, both read from source rather than copied:

- **Guard statuses.** For every `impl FromRequest for G` under the source root, the
  literal statuses in its `Outcome::Error((Status::…)` and `Outcome::Forward(Status::…)`
  arms. An arm whose status is computed (`err.http_status()`) marks `G` _dynamic_:
  the computed code contributes nothing to the required set, its literal codes
  still do, and the dynamic set is pinned so a new one is a decision. Today
  `GuardShare` is the only dynamic guard.
- **Error-kind map.** `enum ErrorKind` variants and the `http_status()` match, read
  from the file passed with `--app-error-map` (default `backend/src/error.rs`):
  named arms directly, unlisted variants through the `_` arm. A body kind that is
  not a variant of that enum is a finding, not a guess.

The `rocket::http::Status` → code table is a fact of the Rocket crate, held as one
commented constant — the same class of external knowledge as B2's `Option`.

| Rule | Required set must be declared                          | Declared must be in the universe                                                         |
| ---- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------- |
| P1   | success status(es) from the return type                | success statuses                                                                         |
| P2   | literal statuses of guards named in the signature      | guard statuses                                                                           |
| P3   | statuses of `ErrorKind::` literals in the handler body | body-kind statuses                                                                       |
| P4   | —                                                      | ∪ (fallible ? every `http_status` code : ∅) ∪ ({400} if the route binds data or a query) |

**P1 — success status.** `success_of` reads the return type: a fallible return (a
`Result` type, or an alias of one derived from `type X = Result<…>` declarations in
the scanned sources) contributes the success of its payload; `Status` is every
`Status::` constant the body returns; `Redirect` is every constructor the body calls
(`to` → 303, `found` → 302, `temporary` → 307, `permanent` → 308, `moved` → 301);
anything else is 200. An unreadable status constant is a finding. Declared success
is compared for equality, so a handler answering 202 that declares 200 both misses
202 (P1) and declares an impossible 200 (P4).

**P2 — guard statuses.** Each argument type's identifiers are looked up in the guard
table and the union of their literal statuses must appear in `responses(…)`.
Signature-only; no body reading.

**P3 — body error kinds.** Every `ErrorKind::K` literal in the body maps through the
error map and must be declared. Body-local only: a status a helper can raise is
deliberately not required, since requiring it would need call-graph analysis. P4's
universe covers those instead, which is the honest limit.

**P4 — declared statuses must be possible.** `declared \ universe` is a finding. The
universe over-approximates on purpose, so a helper-raised error code never flags and
the direction stays false-positive-free. What it does catch: a success code the
handler never returns, an error code on an infallible route with no bindings (the
measured `get_album_index_status` declaring 400), and codes outside every set.

**Fixtures.** `p1_redirect_missing.rs`, `p1_status_return_missing.rs`,
`p1_unreadable_status.rs` / `p1_status_return_conforming.rs`; `p2_guard_missing.rs` /
`p2_guard_conforming.rs`, `p2_dynamic_guard_conforming.rs`; `p3_kind_missing.rs`,
`p3_unknown_kind.rs` / `p3_kind_conforming.rs`; `p4_exotic_status.rs`,
`p4_impossible_route_400.rs` / `p4_route_400_conforming.rs`,
`p4_universe_conforming.rs`.

## Calibration

The router tree has 63 annotated handlers across 30 files. `the_router_tree_is_clean`
pins these counts so a narrowed scan fails instead of reporting a smaller clean
tree:

| Fact                                           | Pinned value |
| ---------------------------------------------- | -----------: |
| Annotated handlers                             |           63 |
| Readable inline declared parameters            |            1 |
| Unreadable declared parameters                 |            0 |
| Declared request bodies                        |           24 |
| Declared body types matching the route binding |           21 |
| Dynamic guard names (computed outcome status)  |            1 |

All A1–A7, B1–B4 and P1–P4 run over the tree with zero findings. The 21 matching
bodies are the 24 declarations less the unconstrained `Value` body and the two form
bodies whose schema is not compared — B4 checks those two form routes' media type
instead. The dynamic guard is `GuardShare`.

The pinned rows are asserted by the test except the last two, which the plan states
and the test does not yet enforce; both are open work below.

Enforcing P1–P4 surfaced 65 findings across 27 files, all fixed in the same change:
19 handlers missing 405 behind `GuardReadOnlyMode`, 32 missing 500 from body
`ErrorKind` literals, 6 missing 404, the two test probes missing 401, the two
`Status::Accepted` album-index handlers declaring 200 instead of 202, and one
impossible 400 removed from the infallible, bindingless `get_album_index_status`.

## Document-level state

Spectral (`@stoplight/spectral-cli`, a `frontend/` devDependency invoked with
`npx --no-install`, ruleset `.spectral.yaml` extending stock `spectral:oas`) is
phase 3 of `just openapi-check`; errors fail the gate, warnings do not. It is
configured and described in `docs/openapi-generator.md`. Baseline is **0 errors, 2
warnings**, both by design: `path-params` on the rank-disambiguated
`/{dynamic_album_id}` vs `/{path}` SPA fallbacks, and `operation-success-response`
on the intentionally always-`401` `GET /unauthorized`.

Established at baseline and still true of `backend/src/openapi.rs`: global `tags`
and `info(description, contact)` are declared; `oas3-api-servers` is off because
the instance is self-hosted and has no canonical URL; `openapi_public.rs` strips the
orphaned `FileEntry` component and the `internal`-tagged probes, which is why 63
annotated handlers publish as 61 operations. The 51 `operation-description` warnings
the phase opened with were cleared by writing a real second doc paragraph for every
handler — auth models, defaults, conflict semantics, side effects. That backfill
surfaced a real defect: `redirect_to_login` declared `302` while Rocket's
`Redirect::to` answers `303`, which is why P1 derives a `Redirect` success from the
constructor the body calls rather than assuming 302.

Parameter descriptions were the next rule proposed for the document layer and are
open work below. The check and the fix are not in the same place, which is why the
ownership decision is still open.

## Steps

| #   | Step                                                                                                                                | Status |
| --- | ----------------------------------------------------------------------------------------------------------------------------------- | ------ |
| 1   | A9 — fail closed on every annotation form the parser does not model                                                                 | open   |
| 2   | P2 — pin how many signature guards resolved, so an unresolved guard fails instead of being skipped                                  | open   |
| 3   | Pin the 21 declared body types matching the route binding, and the dynamic-guard count                                              | open   |
| 4   | Check the facts the checker states: `TAGS` against the docs table, and the utoipa/Rocket assumptions against the generated artifact | open   |
| 5   | Decide the owner of the parameter-description rule, then document the 21 undescribed published parameters                           | open   |
| 6   | Decide the owner of the security-scheme rule, then register schemes and declare `security(...)` per operation                       | open   |
| 7   | Check that a `GuardResult<…>` binding has its rejection propagated                                                                  | open   |

**1 — A9 grammar coverage.** `method(GET)`, `tags([…])` and `context_path` are legal
utoipa spellings the parser skips, so they pass A1 and A3 silently. That is the one
place an enforced rule can be walked past. Fail closed instead: any top-level
annotation argument the parser does not model becomes an
`unsupported_annotation_form` finding at its own line, alongside a failing fixture
and a conforming counterpart for each newly supported spelling. Request-body forms
the parser cannot read route here too, rather than being treated as verified by B3.

**2 — P2 guard coverage.** `guard_codes.get(ident)` returning `None` skips the guard
in both P2 and P4 (`lib.rs:2297`, `lib.rs:2408`), and nothing counts the skips. The
guards resolve today only because all eight `FromRequest` impls happen to sit in
`backend/src/router/auth.rs`, inside `--source-root`. A crate alias such as
`GuardResult` is expected to be skipped; a guard whose impl moved out of the tree is
not. Move one and P2 goes quiet with no finding. Pin the resolved set the way the
dynamic set is pinned.

**3 — calibration rows.** `HandlerSummary` carries declaration counts only. It has
no counter for bodies actually compared against a binding, so B3's comparisons can
shrink to nothing — every declaration unreadable, or every binding non-`Json` — and
the 21 row still holds in prose. The dynamic-guard row is pinned as a name set but
not as a count. Both need one field each on `HandlerSummary`.

**4 — stated facts.** Two gaps against the three conditions above, both cheap to
close. `TAGS` and the "Tag conventions" table in `docs/openapi-generator.md` are
kept in agreement by a test that reads the document and compares the vocabulary —
that turns the acknowledged copy into a checked mirror, and a new tag can no longer
be added to one and forgotten in the other. The third-party assumptions are better
expressed as assertions over the generated artifact than as prose: that every
operation's `operationId` equals its handler name (A6's premise), that a published
parameter's `required` matches the `Option`-ness of the handler argument (B2's), and
that a `Redirect::to` handler publishes 303 (P1's). `backend/src/tests/openapi_contract.rs`
already reads the public spec for parity and uniqueness, so this is the same
mechanism. A dependency bump that changes a derivation then fails a named test.

**5 — parameter descriptions.** The document publishes 22 parameters and describes
one, `POST /upload :: auto_rename`; the other 21 carry a name and a schema and
nothing else. `rocket_extras` derives the list from the route attribute and utoipa
takes a description only from `params(…)`, so each fix is a `params(…)` entry —
exactly one `params(` exists in the tree today. A parameter with a closed value set
should also name those values. Decide the owner first: Spectral detects, the source
gate pins the count, and the fix lives in the annotation. Operation `description`
is a different field and is done — all 61 operations carry a summary and a
description.

**6 — security schemes.** The document registers no `securitySchemes` and none of the
61 operations declares `security`, so a generated client cannot tell that an
operation requires authentication. Scheme registration is a document fact in
`backend/src/openapi.rs`; the guard-class to scheme-name mapping is a stated
project table like `TAGS`, and under this plan it earns its place by meeting the
three conditions above rather than by avoiding them. Both directions are checkable
against the eight `FromRequest` impls. This belongs to the document layer rather
than to section P, and `.plan/authz-check.md` (`backlog`, `high`) already claims the
guard-modelling territory — settle ownership before building.

**7 — guard propagation.** `GuardResult<GuardAuth>` is a parameter in 18 handler
signatures. A `let _ = auth;` discards the rejection, so the route enforces nothing
while the document declares `401`. This had a proven incident
(`.plan/bug-get-rows-auth-guard-discarded.md`, `done`) and nothing checks it now. The
argument parsing P2 already does is the whole mechanism.

## Not doing

- **Generated route-group mounting.** All five `generate_*_routes()` are mounted
  (`backend/src/router/builder.rs:93`), and a forgotten `.mount()` already surfaces
  as documented-but-unmounted drift in `--check-openapi`. Identifying the missing
  group directly is diagnostic polish, not a gate.
- **`POST` routes under `/post/`.** Two routes already sit outside the family
  (`/get/prefetch`, `/upload`), so the rule needs exceptions, and the exception
  mechanism would be a new attribute convention invented to serve a lint.
- **`pages` tag only in `get_page.rs`.** Re-derives route family from file path to
  police one vocabulary value A3 already holds.
- **Statuses raised inside helpers.** Covered by P4's universe; requiring them
  directly needs call-graph analysis.

## Execution and acceptance

`openapi-sanity` is phase 1 of `just openapi-check`, scanning `backend/src/router`.
The recipe passes `--expect-at-least 60`, a floor below the measured 63, because a
walk that stopped descending produces exactly the report a clean tree produces.
The exact count is pinned in `the_router_tree_is_clean`, which is what catches a
lost handler. Findings are reported as `file:line: handler: message`; exit `0` is
clean, `1` is findings or a scan below the floor, `2` is unreadable input.

Fixtures live in `utils/openapi-sanity/tests/fixtures/openapi_annotations/` and are
loaded with `include_str!`; the app-error map fixture is
`tests/fixtures/app_error_map.rs`, shaped like `backend/src/error.rs`. Each enforced
rule needs a failing case, and a conforming counterpart where one exists. Backend
changes run `just openapi-check` and `just utils-test` in pre-commit; CI runs
`just check` and `just test`.
