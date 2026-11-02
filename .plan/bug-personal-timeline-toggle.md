---
status: open
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
2. Fix at the handler: read the new value from the model instead of
   the `change` event (e.g. `@update:modelValue`, or read the
   `includePersonal` ref after the model updates) — one-line-class
   fix, no new plumbing.
3. Re-run the toggle e2e steps green, then the full personal-albums
   e2e set plus `just frontend-check` / `just frontend-vitest`.

## Out of scope

Grid filename chips default off (separate observation from the same
F9 pass; test-design constraint, not a product defect — filename
`text_visible`/`text_hidden` assertions on tiles are vacuous either
way until chips are enabled, so F9 toggle assertions must use a
signal that actually renders, e.g. tile presence via `ui.count`, or
enable chips through the real settings UI in-test).
