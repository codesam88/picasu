# openapi-sanity

Source-level checks on the backend's `#[utoipa::path]` annotations.

This is a **new tool that reuses the name of the analyzer deleted in `e1ec74da`
on purpose.** It is not that analyzer, it does not carry any of its rules, and
none of them are coming back: there is no tag policy, no `AUTH_POLICY` table, no
route-set rule and no source/spec comparison here. Invoking it expecting the old
rules gets you nothing it used to check — in particular no check on tags,
security schemes or whether a documented operation is mounted. Its rule set is
[`.plan/openapi-annotation-checks.md`](../../.plan/openapi-annotation-checks.md).

## What it asserts

Nothing yet. This increment is the tool: it walks the source tree, collects the
`#[utoipa::path]`-annotated handlers, and asserts that the walk saw the whole
tree. Each rule of that plan lands in a later increment, with the fixture that
must fail when the rule is deleted.

Only annotated handlers are in scope; an undocumented route is a contract finding
elsewhere. A rule reads the source with `syn`, because nothing in the generated
document or in Rocket's mount table says what an annotation declares.

## What it deliberately does not check

- **Route coverage** — whether every mounted route is documented, and whether
  every documented operation is mounted. That is `just openapi-routes-match`
  (`--check-openapi`), which compares a real build's route table with
  `backend/openapi.json`.
- **Document validity** — whether the committed document matches a fresh
  generation from the annotations. That is `just openapi-json-match` and
  `just openapi-gen`.
- **Tags, security schemes and the rest of the deleted rule set** — not
  reintroduced here. Where a future rule lands is recorded in the plan file.

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

`tests/openapi_annotations.rs` carries the fixtures for both rules plus a run
over the real router tree that has to stay silent. `tests/cli.rs` pins the
reporting contract the recipe depends on: the summary's counts, the exit codes and
the coverage floor. Fixtures live in `tests/fixtures/openapi_annotations/` and are
pulled in with `include_str!`, so a renamed or deleted fixture breaks the build
instead of skipping a test. Run them with `cargo test -p openapi-sanity`.
