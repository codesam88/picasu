---
status: open
type: bug
priority: medium
area: backend
---

`POST /get/prefetch` declares `request_body = serde_json::Value`
(`backend/src/router/get/get_prefetch.rs:280`), which utoipa publishes as an
empty schema — "any JSON value". The route binds
`data = "<query_data>"` on a `query_data: Option<Json<Expression>>`
(`get_prefetch.rs:287-291`), so what the server actually parses is the filter
grammar of `backend/src/model/expression.rs`: a fifteen-variant enum (`Or`,
`And`, `Not`, `Tag`, `ExtType`, `Ext`, `Model`, `Make`, `Path`, `Album`,
`RootAlbum`, `ParentAlbum`, `Trashed`, `Any`).

The gap is precision, not correctness: nothing the server accepts is rejected by
the document, but a client generated from it cannot know what a valid prefetch
body looks like, and every body it sends is a guess until the server answers
`400`. The frontend has the grammar in TypeScript
(`frontend/src/api/fetchPrefetch.ts` and the expression builder that produces it),
so the knowledge exists — it just is not in the document.

`request_body = serde_json::Value` is **not** a finding under
`.plan/openapi-annotation-checks.md` B3, and deliberately so: B3 treats utoipa's
`Value` as _declares no constraint_, because it constrains nothing the route
could contradict. That is a statement about the rule, not about this annotation
— this annotation is under-specified rather than wrong.

## Notes

Two ways to fix it, and the choice is a public-API decision rather than a
tooling one.

1. **Publish the grammar.** Add `#[derive(utoipa::ToSchema)]` to `Expression`,
   `FilterValue` and `AlbumFilterValue` in
   `backend/src/model/expression.rs` and declare `request_body = Expression`.
   The recursion (`Not(Box<Expression>)`, `Or(Vec<Expression>)`) is handled by
   utoipa; `ArrayString<64>` in `AlbumFilterValue::Value` and `ParentAlbum` is
   the one field utoipa does not know, and needs a `value_type = String`
   override. The cost is a large public schema — three new entries in
   `components.schemas` and a `$ref` where an empty schema was — which commits
   the filter grammar's variant names to the public API surface. Renaming a
   variant becomes a breaking change to the published document.
2. **State the constraint in prose.** Keep `request_body = Value` and describe
   the grammar in the operation's `description`, which utoipa derives from the
   handler's doc comment (the existing one-line summary plus a second paragraph).
   Nothing new reaches `components.schemas`, and no variant name is committed, but
   a generated client still gets no machine-readable shape and has to read prose.

Whichever is chosen, the fix belongs in this file and not in the B3 rule: B3's
calibration says a declared body must equal the type the route binds, and
`Value` is the documented spelling of "this operation does not constrain its
body". If option 1 is taken, B3 will then hold `prefetch` on its own — no
exemption is needed for that.

Reference: this task is **Q2** in
`.plan/openapi-annotation-checks.md`, next to **Q1** (the deliberately-public
routes behind D7/D8).
