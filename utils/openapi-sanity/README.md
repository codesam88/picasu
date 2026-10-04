# openapi-sanity

Source-level checks on the backend's `#[utoipa::path]` annotations.

This is a **new tool that reuses the name of the analyzer deleted in `e1ec74da`
on purpose.** It is not that analyzer, it does not carry any of its rules, and
none of them are coming back: there is no `AUTH_POLICY` table, no route-set rule
and no source/spec comparison here. Invoking it expecting the old rules gets you
the rules below and nothing else — in particular no check on security schemes or
on whether a documented operation is mounted. Its rule set is
[`.plan/openapi-annotation-checks.md`](../../.plan/openapi-annotation-checks.md),
of which **seven rules are implemented** — A1–A7 — and the rest are specified
but not built; both lists are below.

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

**Nine rules, all of them enforced, and nothing else.** Each one below gives what
it asserts, **what specifically tests it** (fixture and test name, so a reader can
go and change the test rather than guess; fixture paths are relative to `tests/`),
and **why it exists** — the defect it
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

- **Tested by** `a_multi_line_summary_fails` over `a5_multi_line_summary.rs`,
  `an_empty_summary_fails` over `a5_empty_summary.rs`, and by
  `a_one_line_summary_with_a_wrapped_description_is_accepted` over `a5_conforming.rs`.
- **Why it exists** — that paragraph is the `summary`, and the generated
  reference renders the `summary` as a heading, so a wrapped paragraph puts a
  newline inside a markdown heading. Measured on this repository's own document,
  not adopted as a style opinion; it found 8 findings. An empty first paragraph
  is also a finding because it leaves utoipa with no summary text to render.

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

## Specified but not built

The tool is **not** the whole plan. These rules are written down in
[`.plan/openapi-annotation-checks.md`](../../.plan/openapi-annotation-checks.md)
with their reasoning, their calibration and where they are meant to land; none of
them is enforced here, and the tree passing says nothing about them.

| id  | rule, in one line                                                                                                                                  |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| B1  | every declared `params(...)` name corresponds to a route segment or query binding                                                                  |
| B2  | a declared parameter's `required` equals `!argument_is_option`                                                                                     |
| B3  | a declared `request_body` schema equals the type the route's `data = "…"` binds                                                                    |
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
cross-reference for the deferred guard-propagation work,
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
- **Parameter agreement (B), the guard/`security` rules (D, M), the tag/route-family
  rule (S1) and the naming rule (A8)** — not implemented, and listed by id above so
  a reader knows the tool is not the whole plan. Where each one lands and what it
  is calibrated against is in the plan file.

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
rather than against snippets, and it pins the scan's coverage (63 annotations) so the two cannot drift apart
silently.
`tests/cli.rs` pins the reporting contract the recipe depends on: the summary's
counts, the exit codes and the coverage floor — five tests there, 15 here, 20 in
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
