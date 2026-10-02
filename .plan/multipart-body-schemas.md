---
status: open
type: feature
priority: medium
area: backend
---

The two multipart routes name their media type but say nothing about what is in
the body. Both declare

```rust
request_body(content_type = "multipart/form-data", content = Object)
```

— `backend/src/router/post/post_upload.rs:153` and
`backend/src/router/put/regenerate_thumbnail.rs:33` — and `Object` is utoipa's
"an object with no declared properties". So the committed document publishes

```json
"multipart/form-data": { "schema": { "type": "object" } }
```

for both. A client reading that knows it must send `multipart/form-data` and
nothing else: it cannot know which fields the endpoint wants, which are
required, or what types they take. It has to read the source.

## What the routes actually parse

| operation                                  | binding                                                                                         | fields                                                                            |
| ------------------------------------------ | ----------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `POST /upload`                             | `form: Result<Form<UploadForm<'_>>, Errors<'_>>` (`post_upload.rs:167-171`)                     | `file` (`Vec<TempFile<'r>>`, `post_upload.rs:25-33`), `lastModified` (`Vec<u64>`) |
| `PUT /put/regenerate-thumbnail-with-frame` | `form: Result<Form<RegenerateThumbnailForm<'_>>, Errors<'_>>` (`regenerate_thumbnail.rs:41-45`) | `asset_id` (`String`), `frame` (`TempFile<'r>`) — `regenerate_thumbnail.rs:20-28` |

Both field names come from Rocket's `#[field(name = "…")]`, not from the Rust
field names, which is a detail a hand-written inline schema would have to
reproduce: `files` is sent as `file` and `last_modified` as `lastModified`.

Two things about this shape make it harder than it looks:

- **The payload carries a lifetime.** `UploadForm<'r>` and
  `RegenerateThumbnailForm<'r>` hold a `TempFile<'r>`, which borrows the request.
  There is no plain type an annotation could name, which is why **B3 does not
  compare a `Form<…>` binding at all** — see "B3's two limits" in
  `.plan/openapi-annotation-checks.md`.
- **`TempFile` has no `ToSchema`.** Giving either form a schema means writing an
  impl for a struct that holds one, or hand-writing the object schema inline.
  `TempFile` is Rocket's; there is no utoipa impl to derive, and the file part is
  a binary stream, so its "type" is `string`/`binary` with a `format`, which is a
  decision about how a generated client should send it rather than a derivation.

## Why this is not B4

B4 (`a `Form<…>`binding declares`multipart/form-data``) is about the **media
type**, and it is enforced. It found the real defect: both routes previously
declared `request_body = Value`, so the document published
`application/json` with an empty schema for two multipart upload endpoints. Both
annotations were fixed in the change that wrote B4, and
`backend/openapi.json` moved in exactly those two media types.

What B4 could not check, and what is left here, is the **schema inside** the
multipart body. So the two pieces are separate work with separate owners:

| piece          | rule | state                       |
| -------------- | ---- | --------------------------- |
| the media type | B4   | enforced, green             |
| the fields     | none | under-specified — this file |

Nothing in the current rule set can catch a regression here, and adding one
would mean reimplementing utoipa's multipart handling in the checker, which is
the thing the tool's module docs warn against ("if a rule seems to need …, the
rule belongs in the backend or in a just recipe"). So this is a backend change
first and, at most, a rule second.

## What a fix looks like

Three ways, none of them a one-liner:

1. **`#[derive(ToSchema)]` on the form structs**, with the `TempFile` field
   annotated as a binary string — `#[schema(value_type = String, format = Binary)]`
   or an equivalent override — and the lifetime handled by a
   `#[schema(...)]`-level ignore. Both structs then appear in
   `components.schemas` and the annotation names them. This is the honest answer:
   the schema is derived from the struct the route parses, so it cannot drift.
2. **Hand-write the object inline**, in utoipa's `inline(SchemaObject)` form,
   declaring the four field names and their types without a `ToSchema` impl. Less
   new public surface, but the schema becomes a second copy of the struct beside
   it, with nothing comparing them — the same rot A1 and A6 exist to prevent.
3. **Split the file part from the metadata.** Serve the binary from its own
   route and leave the form with only scalar fields, which a plain derive can
   describe. The largest change, and the one that would make the schema fall out
   rather than be written.

Option 1 is the one to reach for first. Whichever is chosen, note that the
committed artifact moves, so the change runs `just openapi-gen` and
`just docs-openapi` like any other document change.

## Notes

Reference: this task is **Q3** in `.plan/openapi-annotation-checks.md`, next to
**Q2** (`.plan/bug-prefetch-request-body.md`, the same shape of gap on a JSON
body rather than a multipart one) and **Q1** (the deliberately-public routes
behind D7/D8).

Recorded 2026-10-03, in the change that built A8 and C3. Neither rule touches
this: A8 reads a binding's name and C3 reads the statuses an annotation declares,
and a media type and an itemised schema are a different pair of facts from both.
B4 covers the media type and is already green, so nothing in the current gate
would notice a regression here — which is the reason this is filed as its own
item rather than left as a footnote to B4's entry.
