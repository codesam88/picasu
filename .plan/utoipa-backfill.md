---
status: done
type: feature
priority: medium
area: testing
---

utoipa annotation backfill for backend routes:

1. Annotate one route as a template, confirm `openapi.json` regenerates
2. `cargo xtask openapi-coverage` — routes vs spec, percentage + unannotated list
3. Backfill remaining routes incrementally, one per PR

Currently: 3 routes annotated (authenticate, get_albums, assign_album) behind `openapi` feature flag. `openapi.json` exists but most routes unannotated.
