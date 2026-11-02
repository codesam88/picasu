---
status: done
type: bug
priority: high
area: frontend
---

## Problem

The timeline's "Include personal items in timeline" switch never takes
effect. `TimelinePage.vue` binds the v-switch with `v-model` but
handles `@change="onToggle"`, and a native `change` listener receives
the DOM `Event`, not the new boolean — so `onToggle`'s
`value === true || value === 'true'` check is always false and it
unconditionally calls `setIncludePersonal(false)` before refetching
the gallery. Flipping the switch ON writes `false`
(`frontend/src/script/utils/personalTimelineToggle.ts` stores opt-in
as a localStorage flag, absent by default) and refetches with
`include_personal=false`. The switch UI itself still flips via
`v-model`, so the control lies: it looks on while personal items
never appear. Found by the F9 e2e-honesty pass while replacing
wait-then-shell-visible steps with real toggle assertions
(`.plan/f9-e2e-honesty.md`); confirmed by code reading plus a
diagnostic browser run logging the received `Event` object.

This is the sole control for showing personal items in the timeline
(opt-in by design), so the personal-timeline path is silently dead
until fixed — no error, no log, just missing photos.

## Goal

Flipping the switch on shows personal items; flipping it off hides
them; the persisted flag matches the switch position.

## Steps

1. Pin the behavior first (RED): driving the actual switch control
   on must make a seeded personal photo appear in the timeline grid
   (content assertion, not shell visibility); off must remove it.
   The current code fails the ON half.
2. Fix at the handler and relocate the control: move the "Include
   personal items in timeline" switch out of its prominent timeline
   position into the settings UI next to the "Show Filename Chip"
   toggle (same pattern, same place users manage display
   preferences), wired with `@update:model-value` — the event the
   component actually emits with the new boolean (verified in the
   installed Vuetify source: `VSelectionControl` declares only
   `update:*` emits, so `@change` falls through to a native DOM
   listener and the handler receives an `Event`, for which
   `=== true` / `=== 'true'` is always false and
   `setIncludePersonal(false)` runs on every flip). The handler's
   `boolean | string | null` signature then receives what it was
   written for; keep `v-model` for the visual state as today.
3. Re-run the toggle e2e steps green, then the full personal-albums
   e2e set plus `just frontend-check` / `just frontend-vitest`.

## Result (2026-11-02)

Fixed production binding: `FrontendConfig.vue` now uses `:model-value` + a single
`@update:model-value="onToggle"` handler (owning ref update, persist, and
refresh). `TimelinePage` had no stale toggle; the real blocker was that `Gallery.vue`
passed the coerced boolean `false` for `includePersonal` on the timeline route,
overriding localStorage. The timeline now passes `undefined` on the timeline route
so `getIncludePersonal()` (localStorage) governs. F9-2 scenario converted to real
toggle assertions (filename chip enabled, alice-secret visible on / hidden off).
`just frontend-check`, `just frontend-vitest`, and the isolation Playwright
scenario pass.

## Out of scope

Grid filename chips default off — that needs no production change:
it is a pure frontend preference with a real switch in the frontend
config UI (`FrontendConfig.vue`, correctly wired via
`@update:model-value`), so honest e2e assertions enable chips
through the settings UI in-test and then assert on the rendered
filename. No default change, no new signal needed.
