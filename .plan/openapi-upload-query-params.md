---
status: done
type: feature
priority: low
area: backend
---

Document the `/upload` query parameters in the generated OpenAPI document.

## Final state

All three parameters the route binds are published in `backend/openapi.json`:
`auto_rename`, `on_conflict`, and `presigned_album_id_opt`. `rocket_extras` derives
them from the route attribute, so `#[utoipa::path]` only needs an inline entry when
it adds a description. `auto_rename` has one, including its default and the 400
rejection when sanitization is disabled.

The plan originally listed `replace` as an `on_conflict` value. The route accepts
`skip` and `rename` only, defaulting to `rename`; any other value is a 400.

Descriptions for the two remaining parameters, and for most other published
parameters, are carried as document-linting work in
`openapi-annotation-checks.md`.
