---
status: done
type: feature
priority: medium
area: testing
---

Annotate the backend API surface and generate a committed OpenAPI artifact.

## Final state

Every data route carries a `#[utoipa::path]` annotation, and the document is
generated from the route tree rather than listed by hand. `backend/openapi.json`
is the committed artifact; `just openapi-gen` rewrites it and `openapi-json-match`
in `just openapi-check` fails when it drifts from the generated spec.

The original scope items are superseded rather than implemented as written:

| Original item           | Current mechanism                                                          |
| ----------------------- | -------------------------------------------------------------------------- |
| `cargo xtask` coverage  | Source gate `openapi-sanity`, pinned to 63 annotated handlers              |
| Manual `ApiDoc` paths   | `#[utoipauto(paths = "./backend/src/router")]` in `backend/src/openapi.rs` |
| `widdershins` reference | No generated Markdown; `docs/openapi-generator.md` is hand-written         |
| `just check-api-docs`   | `just openapi-check`                                                       |

Route coverage is enforced from three directions: `openapi-sanity` checks source
annotations against the routes beside them, `openapi-json-match` checks the
artifact, and `openapi-routes-match` compares mounted routes with documented
operations. Source-level rules live in `openapi-annotation-checks.md`.
