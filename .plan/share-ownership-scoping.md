---
status: done
type: feature
priority: high
area: backend
---

## Notes

Per-user share ownership: the main `/shares` page shows only the current
user's shares (admin panel keeps the full list), and the page's property
columns collapse into one `Access` icon column. Decisions from review:
backend scoping (real boundary, not display-only), four icons
dimmed-when-off with tooltips.

## Design (settled)

No install base exists, so the field lands directly on the stored struct
instead of a side table: `Share.created_by` (server-assigned, never taken
from the client). Schema versions restart at v1; everything that only
handled older lineages is removed (pre-removal version tests, v2
assertions, VN-era comments). The version gates themselves stay as
forward-looking corruption/downgrade protection, as do the round-trip and
prefix tests.

- `create_share` stamps the caller id; `edit_share` preserves the stored
  creator across full-object replaces (falls back to the editor on a
  missing entry) — same keep-semantics as the password field.
- `GET /get/get-list` filters each album's `shareList` to the caller's own
  shares. The admin panel does not filter.
- Creator ids never enter API responses (`ShareInfo` unchanged).

## Steps

1. Struct field + write path: `created_by` on `Share`, stamp on create,
   preserve on edit; versions to v1; drop dead lineage handling.
2. Read path: `get_list` filters by caller. Handler tests: non-admin sees
   only own; edit by another admin preserves the original creator.
3. Spec/contract regen (`openapi.json`, matrix) + gates green.
4. Frontend: LinksPage `Access` column (structural pins + e2e text/icon
   assertions on the seeded scenario); keep admin panel as-is.
5. Full suites + live e2e; plan done; PR.

## Resolution (2026-10-31, reworked)

No install base exists, so the side-table approach was replaced before
merge: `Share.created_by` lives on the struct itself (server-assigned on
create, preserved across edits with the editor as fallback), schema
versions restart at v1, and everything that only handled older lineages
is gone (pre-removal version tests, v2 assertions). The version gates
stay as forward-looking corruption/downgrade protection, as do the
round-trip and prefix tests. Full backend suite green; Playwright
share-page-scoping live green.
