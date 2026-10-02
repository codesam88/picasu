# openapi-sanity

Source-level checks on the backend's `#[utoipa::path]` annotations.

This is a **new tool that reuses the name of the analyzer deleted in `e1ec74da`
on purpose.** It is not that analyzer, it does not carry any of its rules, and
none of them are coming back: there is no `AUTH_POLICY` table, no route-set rule
and no source/spec comparison here. Invoking it expecting the old rules gets you
the rules below and nothing else — in particular no check on security schemes or
on whether a documented operation is mounted. Its rule set is
[`.plan/openapi-annotation-checks.md`](../../.plan/openapi-annotation-checks.md),
of which section A and the handler-body rules of section C are implemented so far.

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

### Section A — the annotation's shape

Seven rules, each about one `#[utoipa::path]` annotation or the handler it sits
on.
All of them read a fact that exists only in source: the document is generated
_from_ these annotations, so it cannot disagree with them without a rule here
saying so first.

- **A1** — no `path = "…"` and no bare verb token. With `rocket_extras` enabled,
  utoipa derives the path and the verb from the route attribute, so a
  restatement in the annotation is a second copy of a fact nothing compares.
- **A2** — `responses(…)` is present and declares at least one entry. utoipa
  invents no response, so the omission is silent: the operation documents nothing
  it can answer.
- **A3** — exactly one `tag = "…"`, and it is one of the ten in
  [`TAGS`](src/lib.rs) — this repository's copy of the table in
  [`docs/openapi-generator.md`](../../docs/openapi-generator.md) ("Tag
  conventions"). The tool owns the list because a document linter is not
  adopted, and it is a copy rather than a parse because the document is generated
  from the annotations: reading the vocabulary back out of it would check the
  output against its own input. The rule has no exemptions: every annotated
  operation declares one tag. `internal` is how an operation outside the
  published API — the test-only probes, which the backend strips from the
  committed artifact — says so, as an entry in the list rather than a hole in the
  rule.
- **A4** — the handler carries a doc comment. `summary` and `description` are
  derived from it, so a handler without one is an operation that reaches the
  generated reference with neither.
- **A5** — the doc comment's first paragraph is one line. That paragraph is the
  `summary`, and the reference renders the `summary` as a heading, so a wrapped
  paragraph puts a newline inside a markdown heading. This was measured on this
  repository's own document, not adopted as a style opinion.
- **A6** — no `operation_id = "…"`. utoipa derives it from the function name, and
  a hand-set one is the only name in the document that nothing compares.
- **A7** — no `summary = "…"` and no `description = "…"`. utoipa derives both from
  the doc comment, so a hand-set one is the same prose written twice with nothing
  comparing the copies — and while an annotation may set `summary`, "the first
  paragraph _is_ the summary" is false, which is what A5’s premise rests on. A
  per-response `description` is not this: that is how a status code’s text is
  written, and nothing derives it.

### Section C — the handler body

- **C1** — a `GuardResult<…>` handler argument must have its rejection
  propagated. `GuardResult<T>` is `Result<T, AppError>`: the route hands the
  handler a value that may be a rejection, and the handler is the only place that
  rejection can become an error response. A binding that is dropped without `?`
  or consumed only in positions that discard it (`let _ = ident;`), and a binding
  the body never mentions, are findings.
- **C1b** — a plain `Guard…` handler argument needs nothing in the body and is
  never reported. Rocket runs such a guard during request handling and
  short-circuits on failure, so a handler that correctly ignores the value is
  correct code.

Only annotated handlers are in scope; an undocumented route is a contract finding
elsewhere. Every rule reads the source with `syn`, because nothing in the
generated document or in Rocket's mount table says what an annotation or a
handler body contains.

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
- **Parameter agreement (B) and the security rules (D, M)** — not implemented. The
  rest of the plan, and where each future rule lands, is in the plan file.

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
flagging anything. `tests/cli.rs` pins the reporting contract the recipe depends
on: the summary's counts, the exit codes and the coverage floor. Fixtures live in
`tests/fixtures/openapi_annotations/` and are pulled in with `include_str!`, so a
renamed or deleted fixture breaks the build instead of skipping a test. Run them
with `cargo test -p openapi-sanity`.

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
