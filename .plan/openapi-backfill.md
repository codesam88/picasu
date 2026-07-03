---
status: done
type: feature
priority: medium
area: testing
---

Backfill OpenAPI/utoipa annotations + generate committed API reference docs.

## Current state

3 of ~65 routes annotated (authenticate, get_albums, assign_album), behind
`openapi` feature flag. `openapi.json` exists but covers only those 3 endpoints.
~20-25 routes have meaningful JSON API surface; ~25 SPA page routes
(`get_page.rs`) are HTML-serving and can be omitted.

## Scope

1. Build `cargo xtask openapi-coverage` — source-analysis tool that lists routes
   declared in `router/*/mod.rs` vs routes registered in `openapi.rs`, emits
   percentage and unannotated list. Fails if any data-API route is missing.

2. Add `#[cfg_attr(feature = "openapi", utoipa::path(...))]` annotations to all
   data API route handlers (~20-25 routes).

3. Add `#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]` to their
   request/response structs.

4. Register all paths and schemas in `ApiDoc` (`src/openapi.rs`).

5. Add `just openapi-docs` — chains `cargo xtask openapi-gen` then
   `npx widdershins --summary gallery-backend/openapi.json -o docs/openapi-reference.md`.

6. Add `just check-api-docs` — runs `generate-api-docs` then
   `git diff --exit-code gallery-backend/openapi.json docs/openapi-reference.md`.
   Fails if either generated file changed.

7. Wire `check-api-docs` into `just precommit` on `main` branch (after `just test`).

8. Add `widdershins` to `just install-dev`.

## Files under drift guard

| File                           | Generator                         | Guard                                       |
| ------------------------------ | --------------------------------- | ------------------------------------------- |
| `gallery-backend/openapi.json` | `cargo xtask openapi-gen`         | `git diff --exit-code` in precommit on main |
| `docs/openapi-reference.md`    | `widdershins` from `openapi.json` | same guard — transitively consistent        |

Both are committed to the repo. Editing annotations without committing the
regenerated files causes precommit to fail.

## Sequencing

- Step 1 first: the coverage report makes progress measurable and catches
  missed routes during backfill.
- Steps 2-4 incrementally per area (albums, tags/flags, sharing, config, ingestion).
- Steps 5-8: tooling + enforcement, can be done in parallel with final annotation pass.

## Key considerations

- `get_page::album_page` at `GET /<dynamic_album_id>` is a catch-all that
  conflicts with spec generation — needs a unique path in the utoipa annotation.
- Some SPA-internal routes (`/get/prefetch`, `/get/get-rows`, `/get/get-scroll-bar`)
  are consumed only by the frontend; annotate them but consider schema
  complexity vs usefulness for external consumers.
- widdershins is an npm package; added to `just install-dev` so local
  precommit runs don't depend on network.
- Coverage-report step currently checks registration path count, not
  per-route description/param accuracy — that's covered by the `git diff` guard.
