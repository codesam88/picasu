---
status: done
type: feature
priority: medium
area: frontend
---

## Notes

Replace the Tags page (`/tags`) table with a tag cloud. Decisions settled
with user: Vuetify chip cloud (flex-wrap), page width `compact`, count via
tooltip on hover.

Ported from an identical change first built in a sibling checkout
(`urocissa`, branch `feature/tags-cloud`), where it was verified with
vitest (46/46), prettier/eslint/`vue-tsc` clean, and a passing targeted
Playwright "Initial onboarding" run. Picasu's `TagsPage.vue` base was
byte-identical, so the rewrite ported verbatim — but it must be
re-verified here (picasu has diverged elsewhere: `PageTemplate`
restructure, `getter.ts` renames).

## Problem

`frontend/src/components/Page/TagsPage.vue` renders all tags as a two-column
`v-table` (tag button + raw count). It works but scans poorly; the natural
encoding is size-by-frequency. Data (`tagStore.tags: { tag, number }[]`
from `GET /get/get-tags`) already carries everything a cloud needs; only
presentation changes.

## Solution strategy

Chosen: flex-wrapped `v-chip` (tonal) cloud inside the existing `PageTemplate`
card preset widened from `pane` to `compact`. Font size scaled by
`log1p(count)`, clamped to a fixed rem range. Count shown in a `v-tooltip`
on hover/focus. Click behavior unchanged (`searchByTag(tag, router)`).
Sort stays alphabetical; frequency is encoded in size.

Rejected:

- Typographic pure-text cloud: diverges from Vuetify chip language used in
  `ItemTag.vue`, smaller click targets.
- Bar list: most precise, but not a cloud; possible follow-up toggle.
- Third-party cloud lib: styling fights Vuetify, weak at high tag counts.

## Implementation steps

1. Scaling helper + unit test (TDD).
   Scope: `frontend/src/script/utils/tagCloud.ts`
   (`tagCloudWeight` 0..1 via `log1p`, degenerate range returns 0.5;
   `tagCloudFontSize` linear interp, defaults 0.8/1.6rem) plus
   `frontend/src/script/utils/tagCloud.test.ts` (vitest).
   Done: full `npm test` green.
   Out: no Vue/Vuetify in helper; no new deps.

2. Rewrite `TagsPage.vue` template.
   Scope: `TagsPage.vue` only. `PageTemplate` `width="pane"` to
   `width="compact"`; `v-table` replaced by flex-wrap `v-chip` cloud with
   `v-tooltip` (`N items`); empty state "No tags yet"; `searchByTag`
   click, `tagStore` order and fetch logic unchanged.
   Done: prettier, `vue-tsc --noEmit`, eslint clean.
   Out: no sorting/store/API/route changes.

3. Verify in picasu.
   Scope: repo checks. `prettier --check` on touched files, `vue-tsc`,
   eslint, full vitest; rebuild frontend and run targeted Playwright
   scenario "Initial onboarding" (covers `/tags` assertions).
   Done: all green; any failure recorded here, fixed, re-verified.
   Out: full `just test` only if cheap.

## Progress (newest first)

- 2026-10-31: verified in picasu — prettier/eslint/`vue-tsc` clean,
  vitest 346/346 (44 files), frontend rebuilt, targeted Playwright
  "Initial onboarding" passed (18.0s). No defects. Committed + pushed.
- 2026-10-31: ported 3 files byte-exact from sibling checkout onto picasu
  branch `feature/tags-cloud`; awaiting verification (step 3).
