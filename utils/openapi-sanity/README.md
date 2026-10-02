# openapi-sanity

Source-level checks on the backend's `#[utoipa::path]` annotations.

This is a **new tool that reuses the name of the analyzer deleted in `e1ec74da`
on purpose.** It is not that analyzer, it does not carry any of its rules, and
none of them are coming back: there is no `AUTH_POLICY` table, no route-set rule
and no source/spec comparison here. Invoking it expecting the old rules gets you
the rules below and nothing else — in particular no check on security schemes or
on whether a documented operation is mounted. Its rule set is
[`.plan/openapi-annotation-checks.md`](../../.plan/openapi-annotation-checks.md),
of which **thirteen rules are implemented** — A1–A7, B1–B4 and the two
handler-body rules C1 and C1b — and the rest are specified but not built; both
lists are below.

## What this crate holds: conventions, not backend facts

Every constant in this crate is a **convention someone decided** — a tag
vocabulary, a set of spellings utoipa also accepts — and every rule reads
something written in the `#[utoipa::path]` annotation or in the handler beside
it. This tool holds no facts about the backend, and that boundary is the point:

> If a rule seems to need a route path, a URL prefix, a config value, a feature
> name, a mount table or a constant from the backend, the rule belongs in the
> backend or in a just recipe — not here.

A copy of such a fact in this crate is a second place to forget, and a gate built
on it reports a stale copy rather than the truth. It happened once: A3 briefly
carried the backend's contract-exclusion prefixes so the test-only probes could
be exempt from the tag rule, and the honest resolution turned out to be a
vocabulary entry (`internal`) rather than a copy of a path list. Route-set parity,
document drift and feature gating all have owners elsewhere, and none of them is
this crate — the "what it deliberately does not check" list below is the
concrete form of that.

## What it asserts

**Thirteen rules, all of them enforced, and nothing else.** Each one below gives
what it asserts, **what specifically tests it** (fixture and test name, so a
reader can go and change the test rather than guess; fixture paths are relative to
`tests/`), and **why it exists** — the defect it
was written for, or the convention it protects. Rule ids are the plan's
([`.plan/openapi-annotation-checks.md`](../../.plan/openapi-annotation-checks.md),
"Rule index"), and they are stable.

All of them read a fact that exists only in source: the document is generated
_from_ these annotations, so it cannot disagree with them without a rule here
saying so first.

### A1 — the annotation restates neither `path` nor a bare verb

With `rocket_extras` enabled, utoipa derives the path and the verb from the route
attribute, so either one restated in the annotation is a second copy of a fact
nothing compares.

- **Tested by** `a1_restated_path_or_verb_fails` over
  `fixtures/openapi_annotations/a1_restated_route.rs` (all three restatement
  shapes, `path = "…"` and two bare verbs including `trace`), and by the
  conforming counterpart `a_route_annotation_free_of_restatement_is_accepted` over
  `a1_conforming.rs`.
- **Why it exists** — the convention exists only in review; nothing rejects a
  re-introduced duplicate. `trace` is in the verb list because it is a Rocket verb
  and utoipa accepts it as a bare token — the gap was closed after review found
  the list of eight omitted it.

### A2 — the annotation declares at least one response

`responses(…)` is present and declares at least one entry. An absent
`responses(…)` and an empty `responses()` are both findings, reported as
different texts, and the absent one is anchored at the signature because there is
no token to point at.

- **Tested by** `a_missing_or_empty_responses_fails` over `a2_missing_responses.rs`
  (both shapes) and by `one_declared_response_is_enough` over `a2_conforming.rs`.
- **Why it exists** — utoipa invents no response, so the omission is silent: the
  operation documents nothing it can answer.

### A3 — the annotation declares exactly one `tag` from the vocabulary

Exactly one `tag = "…"`, and it is one of the ten in
[`TAGS`](src/lib.rs) — this repository's copy of the table in
[`docs/openapi-generator.md`](../../docs/openapi-generator.md) ("Tag
conventions"). The rule has no exemptions: `internal` is how an operation outside
the published API (the test-only probes) says so, as an entry in the list rather
than a hole in the rule.

- **Tested by** `a_tag_outside_the_vocabulary_fails` over
  `a3_tag_outside_the_vocabulary.rs` (no tag, a tag outside the vocabulary, and
  two tags of which both are inside it) and by
  `every_tag_of_the_vocabulary_is_accepted` over `a3_conforming.rs` — the second
  half of that test builds its source from `TAGS` itself, so a tag added to one
  place and not the other fails the test rather than the gate.
- **Why it exists** — the vocabulary is documented in prose; its checker was
  deleted with the analyzer, and until a document linter is adopted A3 and A6 are
  the only tag/`operationId` coverage this repository has.

### A4 — the handler carries a doc comment

- **Tested by** `a_handler_without_a_doc_comment_fails` over
  `a4_missing_doc_comment.rs` and by `a_doc_commented_handler_is_accepted` over
  `a4_conforming.rs`.
- **Why it exists** — `summary` and `description` are derived from the doc
  comment, so a handler without one reaches the generated reference with neither,
  and nothing in the document shows the omission: an operation with a `paths`
  entry, a `responses` map and a `tags` array is indistinguishable from a
  documented one until a reader looks for prose. **A4 found 50 such handlers on
  this repository's own tree**, and 49 of the 61 published operations had no
  `summary`.

### A5 — the doc comment's first paragraph is one line

- **Tested by** `a_multi_line_summary_fails` over `a5_multi_line_summary.rs` and
  by `a_one_line_summary_with_a_wrapped_description_is_accepted` over
  `a5_conforming.rs`.
- **Why it exists** — that paragraph is the `summary`, and the generated
  reference renders the `summary` as a heading, so a wrapped paragraph puts a
  newline inside a markdown heading. Measured on this repository's own document,
  not adopted as a style opinion; it found 8 findings.

### A6 — the annotation sets no `operation_id`

- **Tested by** `a_hand_set_operation_id_fails` over
  `a6_hand_set_operation_id.rs` and by `a_derived_operation_id_is_accepted` over
  `a6_conforming.rs`.
- **Why it exists** — utoipa derives `operationId` from the function name; a
  hand-set one is the only name in the document that nothing compares, and the
  deleted `AUTH_POLICY` was keyed by it.

### A7 — the annotation sets neither `summary` nor `description`

A per-response `description` is **not** this: that is how a status code's text is
written, and nothing derives it.

- **Tested by** `a_hand_set_summary_or_description_fails` over
  `a7_hand_set_prose.rs` and by `a_derived_summary_and_description_are_accepted`
  over `a7_conforming.rs`.
- **Why it exists** — utoipa derives both from the doc comment, so a hand-set one
  is the same prose written twice with nothing comparing the copies. While an
  annotation may set `summary`, "the first paragraph _is_ the summary" is false,
  which is the premise A5 rests on — `put/assign_album.rs` did set both, and it is
  why A5 was once reporting a defect the generated document did not have.

### B1 — every declared parameter is one the route actually binds

With `rocket_extras`, utoipa derives a parameter for every argument the route
binds and merges whatever the annotation declares on top of it. Nothing checks the
other direction, so a declared parameter no route reads reaches the document: a
generated client sends it and the server ignores it. A `Path` parameter's name must
be a `<segment>` of the route's path; a `Query` parameter's must be a `?<name>` in
its query part. A `<name..>` partial segment binds `name` — the `..` is Rocket's
marker, not part of the name.

- **Tested by** `a_parameter_the_route_does_not_bind_fails` over
  `b1_parameter_the_route_does_not_bind.rs` (both locations, each against the part
  of the route that binds it) and by `parameters_the_route_binds_are_accepted`
  over `b1_conforming.rs` (a query and a path parameter the route binds, a route
  with no query part, and a partial segment).
- **Why it exists** — utoipa merges declared parameters into the derived document
  without checking they exist, and a parameter nothing reads is invisible in the
  document: it looks like a documented input. **Scope:** the inline tuple form
  only. See "What section B does not cover" below.

### B2 — a declared parameter's `required` agrees with the handler argument

utoipa 5.5 has **no `required` key** in a parameter tuple — `required = true` is
rejected by the macro as an unknown attribute, which this repository confirmed by
compiling it — and derives the documented `required` from the declared type alone:
`Option<…>` is optional, anything else is required. So B2 compares the declared
type's optionality with the handler argument's, which is what the document ends up
saying either way.

- **Tested by** `a_declared_optionality_the_argument_disagrees_with_fails` over
  `b2_optionality_the_argument_disagrees_with.rs` (both directions) and by
  `a_declared_optionality_the_argument_agrees_with_is_accepted` over
  `b2_conforming.rs`.
- **Why it exists** — same as B1: utoipa derives the documented optionality from
  the declared type and never looks at the argument the route binds. A declared
  `Option<T>` on a `T` argument tells a client it may omit a parameter the route
  will not parse without; a declared `T` on an `Option<T>` argument tells it must
  send one the route is happy without. Either way the generated client is wrong
  about the call it makes.

### B3 — a declared `request_body` names what the route's `data = "…"` binds

The declared schema must be the type Rocket parses, unwrapping `Json<T>` and
`Data<T>` through any `Option`. Types are compared by the last segment of their
path, which is the name utoipa publishes the schema under.

- **Tested by** `a_body_the_route_does_not_parse_fails` over
  `b3_body_the_route_does_not_parse.rs` (two mismatched declarations) and by
  `a_body_the_route_parses_is_accepted` over `b3_conforming.rs`, which also pins
  the two shapes the rule does not compare.
- **Why it exists** — utoipa takes the declared schema and never compares it to the
  route's binding, so an annotation can advertise a body the route rejects every
  time. **Two stated limits**, both properties of utoipa's grammar rather than
  choices: `request_body = Value` is _declares no constraint_ and is not compared
  (the asymmetry is deliberate — a **named** type on a route binding `Json<Value>`
  is still a finding), and a `Form<…>` binding is not compared at all, because a
  payload carrying `TempFile<'r>` has no schema type an annotation could name.
  B4 covers what can be checked about a form body.

### B4 — a `Form<…>` binding declares `multipart/form-data`

The rule asks the annotation to name the media type rather than reproducing
utoipa's guess for it — the guess is a list of cases (byte arrays are
`application/octet-stream`, primitives are `text/plain`), and reimplementing it
would make this crate a second utoipa to keep in step. Anything but an explicit
`multipart/form-data` is a finding, and so is a form route with no `request_body`
at all.

- **Tested by** `a_form_body_without_multipart_fails` over
  `b4_form_body_without_multipart.rs` (a body declared as `Value`, and no body at
  all, with a JSON route in the same fixture left silent) and by
  `a_form_body_naming_multipart_is_accepted` over `b4_conforming.rs` (both
  spellings utoipa accepts for naming a media type).
- **Why it exists** — **it is the only rule of section B that fires on the real
  tree, and it found a real defect.** `post_upload` and `regenerate_thumbnail`
  bind a `Form<…>` and declared `request_body = Value`, so the document published
  `application/json` with an empty schema for two multipart upload endpoints. Both
  annotations now declare
  `request_body(content_type = "multipart/form-data", content = Object)`, and
  `backend/openapi.json` moved in exactly those two places. The schema is an
  untyped object because itemising the fields needs a `ToSchema` impl for a
  struct holding a `TempFile` — a separate piece of work.

### What section B does not cover

**B1 and B2 read the inline tuple form of a parameter, and the gap is pinned
rather than left silent.** utoipa accepts two spellings in `params(…)`: the
inline tuple `("name" = Type, Location, …)` and a struct — `params(SomeQueryStruct)`,
or a struct mixed with tuples. The struct hides the name, the location and the type
behind a type this tool would have to resolve across files, and **no type in this
repository derives `IntoParams`**, so a resolver would ship untested against real
code. Every entry the rules cannot read is counted in
`HandlerSummary::unread_parameters`, `the_router_tree_is_clean` **pins that count
at 0**, and `only_the_inline_parameter_form_is_read` pins the parsing. The first
struct form in an annotation fails the pin, so the decision happens in review
rather than as a quiet narrowing of the rules.

A declared parameter whose location is neither `Path` nor `Query` is outside B1 as
well: a header or cookie is named nowhere in a Rocket route attribute, so there is
no route binding for it to disagree with.

### C1 — a `GuardResult<…>` argument has its rejection propagated

`GuardResult<T>` is `Result<T, AppError>`: the route hands the handler a value
that may be a rejection, and the handler is the only place that rejection can
become an error response. The binding must appear as the operand of `?`, the
scrutinee of a `match`/`let`/`if let`, an argument of another call, or a returned
value. A binding dropped as `let _ = ident;`, and a binding the body never
mentions, are findings.

- **Tested by** `dropping_a_guard_result_fails` over `c1_discarded_guard_result.rs`
  (the mutation fixture), `a_guard_result_absent_from_the_body_fails` over
  `c1_absent_guard_result.rs`, `propagated_guard_results_are_accepted` over
  `c1_conforming.rs`, `a_dropped_guard_in_an_unannotated_handler_is_out_of_scope`
  over `c1_unannotated_ignored.rs`, `guard_moved_into_a_closure_is_accepted` over
  `c1_moved_into_closure.rs`, and `rebound_guard_result_is_reported` over
  `c1_rebound_guard_result.rs`.
- **Why it exists** — it is the only rule with a proven incident behind it: the
  deleted suite's `dropping_a_guard_result_fails` shape, commit `84f29aa5`, a
  `GuardResult<GuardTimestamp>` dropped without `?`. The tree's conforming idiom
  is `let _ = ident?;` (all 52 fallible bindings use it), and the rule has zero
  findings today, so it earns its place as a regression guard — which is why it
  ships with a mutation test that fails when the rule is removed. The two
  decision-shaped cases are pinned as tests rather than left to the walk:
  a `move` into a closure is **accepted** when the closure propagates, and a
  one-hop rebinding (`let x = auth; … x?;`) is **reported** even though a human
  would accept it.

### C1b — a plain `Guard…` argument needs nothing in the body and is never reported

- **Tested by** `an_unused_plain_guard_is_accepted` over `c1b_plain_guards.rs`
  (a handler with two unused plain guards, and a handler mixing a `GuardResult`
  with a plain one) and by `only_a_guard_result_carries_an_obligation`, which
  pins the classification on `syn::Type` values rather than on a fixture — so a
  new guard type cannot fail the build before someone has decided what it means.
- **Why it exists** — Rocket runs such a guard during request handling and
  short-circuits on failure, so a handler that correctly ignores the value is
  correct code. Nine `_auth: GuardAuth` bindings across seven handlers are never
  touched; a rule that treated every guard type alike would report every one of
  them.

Only annotated handlers are in scope; an undocumented route is a contract finding
elsewhere. Every rule reads the source with `syn`, because nothing in the
generated document or in Rocket's mount table says what an annotation or a
handler body contains.

## Specified but not built

The tool is **not** the whole plan. These rules are written down in
[`.plan/openapi-annotation-checks.md`](../../.plan/openapi-annotation-checks.md)
with their reasoning, their calibration and where they are meant to land; none of
them is enforced here, and the tree passing says nothing about them.

| id  | rule, in one line                                                                                                                                  |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| C2  | every `pub fn generate_*_routes()` is mounted in `router/builder.rs`                                                                               |
| M1  | every `POST`/`PUT` route carries `GuardReadOnlyMode`                                                                                               |
| M2  | a route carrying the mode guard documents a `405` response                                                                                         |
| M3  | the mode guard never appears in `security(...)`                                                                                                    |
| M4  | the mode guard's rejection is propagated                                                                                                           |
| D1  | a route carrying a credential guard has an operation that declares `security(...)`, and the schemes it names are the ones that guard class maps to |
| D2  | an operation declaring `security(...)` has a route carrying a credential guard                                                                     |
| D4  | `securitySchemes` defines every scheme any operation references                                                                                    |
| D5  | a route carrying the re-authentication guard declares its own scheme                                                                               |
| D6  | the re-authentication guard appears with a credential guard, never alone                                                                           |
| D7  | every `POST`/`PUT` route carries at least one credential guard, or is listed as deliberately public — blocked on the backend question Q1           |
| D8  | the credential set an operation declares in `security(...)` equals the credential set its route's guards provide                                   |
| A8  | every guard binding is named after its guard class, `_`-prefixed when the value is discarded — calibrated: 19 of 62 bindings would be findings     |
| C3  | a route carrying a guard documents the status that guard rejects with — `401` for a credential guard, `405` for the mode guard                     |
| R1  | a `POST` route lives under `/post/`, or is listed as deliberately placed elsewhere                                                                 |
| S1  | a handler defined in `router/get/get_page.rs` is tagged `pages`, and no handler defined elsewhere is — calibrated: zero findings today             |

Three ids in the plan are not rules this tool could enforce: **D3** is a
cross-reference (the credential guard's rejection is propagated — that is C1),
**V1** (a credential comparison is constant time) is a review obligation no
checker can see, and **P1** (a handler's documented success status matches what it
returns) is a spike whose return-type analysis cost has to be measured before the
rule is written.

## What it deliberately does not check

- **Route coverage** — whether every mounted route is documented, and whether
  every documented operation is mounted. That is `just openapi-routes-match`
  (`--check-openapi`), which compares a real build's route table with
  `backend/openapi.json`.
- **Document validity** — whether the committed document matches a fresh
  generation from the annotations. That is `just openapi-json-match` and
  `just openapi-gen`.
- **Three spellings utoipa also accepts**, which are review-time rather than
  gated: `method(GET)` is the parenthesised verb form of A1, `tags([…])` is a list
  form of A3, and `context_path` is a base-path form of A1. No annotation uses
  any of them. (`trace` was one of these until it moved into A1’s verb list: it
  is a Rocket verb, so leaving the gap open was the cost of a rule nothing
  enforces.)
- **The guard/`security` rules (D, M), the tag/route-family rule (S1) and the
  naming rule (A8)** — not implemented, and listed by id above so a reader knows
  the tool is not the whole plan. Where each one lands and what it is calibrated
  against is in the plan file.

## Running it

```sh
cargo run -p openapi-sanity                                  # backend/src/router
cargo run -p openapi-sanity -- --source-root path/to/tree   # anything else
cargo run -p openapi-sanity -- --expect-at-least 60         # coverage floor
```

The default source root is relative to the workspace root, which is the working
directory cargo runs rustc in.

One `file:line: message` per finding, a summary line naming the count, and an
exit code of `0` clean, `1` findings or a scan below the floor, `2` an input that
cannot be read.

`--expect-at-least <n>` is the tool's own blindness check, and the gate phase
passes it. A scan that saw fewer annotated handlers than the floor exits
non-zero with a message saying the walk is the likely cause, because a walk that
stopped descending produces the same report as a clean tree — reporting that as
clean is the failure this tool exists to prevent. The floor the recipe sets is 60
against a tree of 63: a new handler must not break the gate, but a lost one should
be noticed. Coverage outranks findings in the same run.

It is the first phase of `just openapi-check`, so it runs on a `backend/` change
in the pre-commit hook — which is why it is a tool and not a backend test. The
hook also runs `just utils-test` for a `utils/` change, so a change to the rules
or to the walk is exercised by its own suite there.

## Tests

`tests/openapi_annotations.rs` carries the fixtures for every rule plus a run over
the real router tree that has to stay silent. Each rule has a fixture that must
produce its finding and a conforming counterpart that must produce none, so a
rule that started flagging every annotation is caught as well as one that stopped
flagging anything — which fixture and which test cover which rule is written down
per rule in "What it asserts" above, so a reader does not have to go looking.
`the_router_tree_is_clean` is the half that proves the rules against the tree
rather than against snippets, and it pins the scan's coverage so the two cannot
drift apart silently:

| pinned                     | value | what a change of it means                                                          |
| -------------------------- | ----- | ---------------------------------------------------------------------------------- |
| annotated handlers         | 63    | the walk stopped finding annotations                                               |
| `GuardResult<…>` bindings  | 52    | C1's calibration moved                                                             |
| plain `Guard…` bindings    | 9     | C1b's calibration moved                                                            |
| declared parameters read   | 1     | B1/B2 read less of the tree than they were calibrated against                      |
| declared parameters unread | 0     | an `IntoParams` struct reached an annotation — see "What section B does not cover" |
| declared request bodies    | 24    | B3/B4 read less of the tree than they were calibrated against                      |

`tests/cli.rs` pins the reporting contract the recipe depends on: the summary's
counts, the exit codes and the coverage floor — five tests there, 32 here, 37 in
all. Fixtures live in `tests/fixtures/openapi_annotations/` and are pulled in with
`include_str!`, so a renamed or deleted fixture breaks the build instead of
skipping a test. Run them with `cargo test -p openapi-sanity`.

## A known duplication

`TAGS` is the one set this tool enforces that is written down elsewhere: it
mirrors the table in
[`docs/openapi-generator.md`](../../docs/openapi-generator.md) ("Tag
conventions"), which cannot be read rather than copied because the document is
generated from the annotations a rule would then be checking. Both places say the
other exists, and a test asserts every tag in the constant is accepted, so a
subject added to one and not the other shows up as a rule that rejects the table.

The guard rules in section C name the shapes `GuardResult<…>` and `Guard…` by the
spelling the backend writes them with. That is a convention read from the
convention, not a fact copied from the backend: a guard class the tool cannot name
is out of scope for every rule here, which is recorded in the plan file as the
design requirement for section D.
