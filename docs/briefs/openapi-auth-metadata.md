# OpenAPI Auth Metadata

How per-operation authorization metadata can live in a utoipa-driven
OpenAPI document: extensions, guard derivation, and what downstream
tooling preserves.

## Vendor extensions (`x-`)

OpenAPI permits `x-`-prefixed properties on most objects with
arbitrary values; strict validators accept them and they round-trip
through serializers. utoipa exposes them per operation via
`extensions(("x-name" = json!(...)))` — the key must be spelled in
full. This project already uses the pattern once (`x-picasu-feature`
for the route-set parity gate), so a second key for auth metadata
follows precedent, not new machinery.

Downstream behavior, verified against tool docs:

- **Ignored but preserved** by docs renderers (Redoc, Swagger UI,
  Scalar read curated `x-` sets and pass the rest through) and by
  Spectral's stock ruleset. A custom Spectral rule could validate
  extension shape via JSONPath, but source-level shape checks belong
  in the syn checker, which sees the annotation before generation.
- **Consumable** by generators: OpenAPI Generator, APIMatic, and
  oapi-codegen all expose `vendorExtensions` to templates, and each
  defines its own `x-` vocabulary — precedent that tool-specific
  metadata in `x-` fields is legitimate, with the vocabulary
  documented once (the way `tags.json` documents the tag set).
- **Risk**: a pipeline step that re-serializes through a strict
  generator could drop unknown keys. The current pipeline
  (annotations → `openapi.json` → Spectral / `--check-openapi` /
  Schemathesis) has no such step.

## Security requirements (standard, keep)

Per-operation `security(...)` with AND/OR grouping and `security: []`
for explicitly public operations is the correct OpenAPI idiom; utoipa
generates it, schemes register via the `Modify` addon (utoipa's
`components(...)` has no `securitySchemes` entry). The gap was never
generation — it is validation that each clause agrees with the
handler's guard signature, which no checker currently does.

## Guard-trait derivation (strongest guarantee, wrong framework)

`rocket_okapi` derives security from guard types via
`OpenApiFromRequest`: each guard's impl declares its scheme, so
handler signatures inherit security with no per-op annotation. That
eliminates annotation drift at the cost of one impl per guard type
(8 here) — and of replacing utoipa entirely, since the two
integrations are different frameworks. The cheaper equivalent is a
syn rule that checks each route's `security(...)` against its guard
signature: most of the alignment benefit, no framework migration.

## Links and overlays (orthogonal / avoid)

Link objects model stateful follow-ups (create → fetch, authenticate
→ use token) with runtime expressions and are already available in
the annotation reference; they carry no security semantics. Overlays
inject transformations (including `x-` fields or security) without
touching source, but support is fragmented across tools and the
result depends on a post-processing step outside the pipeline. Both
are wrong vehicles for auth policy that must track handler guards;
overlays fit environment-specific deployment concerns instead.

## Custom inert attribute (redundant)

A second annotation channel (e.g. `#[picasu_auth(...)]`) that utoipa
cannot read would need translation into `security(...)` or
`extensions(...)` to reach the document — strictly redundant with
writing the extension directly, plus a proc-macro crate to own.

## Pointers

- utoipa path attributes (security + extensions):
  <https://docs.rs/utoipa/latest/utoipa/attr.path.html>
- utoipa `SecurityRequirement` (AND/OR semantics):
  <https://docs.rs/utoipa/latest/utoipa/openapi/security/struct.SecurityRequirement.html>
- utoipa `SecurityScheme` enum:
  <https://docs.rs/utoipa/latest/utoipa/openapi/security/enum.SecurityScheme.html>
- OpenAPI extensions practice: <https://www.speakeasy.com/openapi/extensions>
- Per-operation vs global security: <https://www.speakeasy.com/openapi/security>
- rocket_okapi guard-derived security: <https://crates.io/crates/rocket_okapi>
- Arazzo and overlays: <https://zuplo.com/blog/openapi-arazzo-overlay-specifications-guide>
- Redocly contract testing from OpenAPI (Arazzo/Respect):
  <https://redocly.com/blog/api-contract-testing-arazzo>
- utoipa + Schemathesis contract validation precedent:
  <https://identeco.de/en/blog/generating_and_validating_openapi_docs_in_rust/>
