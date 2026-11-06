---
status: open
type: feature
priority: high
area: frontend
---

## Notes

Overall structural rework of the frontend: consistent design of pages,
modals, titles, tables, buttons, icons, and tooltips, with automated
enforcement and behavior tests. Tool knowledge:
`docs/briefs/vue-structure-vitest.md` (canonical; supersedes the spike's
`vitest-frontend-testing.md`, which prescribed the wrong ban rule — see
step 2). Prior spike (branch `spike/vue-structure-enforcement`)
inventoried the current state and prototyped a structural scan.

Starting position: all top-level pages render through `PageTemplate`;
`BaseModal`/`ShareModalBase` exist but most modals still use raw
`v-dialog`; `Page/Config/*` sections hand-roll cards and inline confirm
dialogs.

## Steps

1. Port the spike's structural scan (`designStructure` rule + test) as
   the adoption gate, keeping the grandfather list for known debt.
   Re-audit the list against this tree first: the spike scoped to
   `Modal/` + top-level `Page/`, but inline confirms also exist in
   `Page/Config/*`, `ServerFilePicker.vue`, and
   `Gallery/GalleryEmptyCard.vue`, while `UploadModal`/`DropZoneModal`
   may already be migrated. Decide narrow vs expanded scope up front.
2. Lint-layer enforcement (earlier feedback than tests). Corrected per
   the new brief: ban raw template usage with
   `vue/no-restricted-html-elements` (element/component names in
   templates) — NOT `vue/no-restricted-component-names`, which targets
   the declared `name` property and would silently pass. Add
   `vue/no-restricted-block` for SFC blocks,
   `vue/no-bare-strings-in-template` for naked copy, plus free
   convention rules (`attributes-order`,
   `require-explicit-emits`/`require-explicit-slots`). Order matters:
   the shared confirm dialog and section card (step 3) must exist
   before their bans land, so authors have a correct target.
3. Migrate the debt: raw-dialog modals to `BaseModal`, inline confirms
   to a shared confirm dialog, Config sections to a shared section
   card; shrink the grandfather list to zero, then delete it.
4. Behavior tests: browser test project (`vitest-browser-vue`,
   pinned against installed Vitest 5.0.5, not `main` docs) with the
   modal accessibility contract (role, focus, `Escape`) and page
   template slots/loading states first. Vuetify pinning: repo is on
   Vuetify 4.3.1 (default theme `system`, changed breakpoint
   thresholds); verify against the v4 testing docs
   (`vuetifyjs.com/en/getting-started/unit-testing/`), pass an explicit
   `theme: { defaultTheme: 'light' }` in the test `createVuetify` for
   determinism, and only then adopt the brief's jsdom recipe.
5. Later, if the layering keeps being violated: `eslint-plugin-boundaries`
   sketch, theme-token lint, visual regression coverage.
6. Test cleanup: detect and remove vacuous absence-only tests, then add
   mutation testing as a periodic audit.

Evidence (2026-10-30 audit of `frontend/src`, 276 test blocks):
75 negations, 18 negation-only blocks. Two families: behavioral
negatives with real stimulus (mock never called after driving the
code — `passwordChange`, `registration`, `tokenStore`,
`axiosInterceptor` — legitimate, falsifiable) versus source-text
absence-only checks (`shareLoginModal` snackbar, `changePasswordForm`
alias, `createShareModal` snackbar, `userManagement` navigation,
`registerRoute` S7 probe/cache — pass even if the feature is deleted).
History shows absence checks arriving with features, plus one commit
explicitly removing vacuous ones and an existing R31 convention
marking source tripwires as rewrite candidates once behavior tests
exist.

Planned mechanisms (not yet built):

- Meta-test in the style of the structural scan: flag test blocks with
  zero positive assertions inside source-reading test files (files
  using `readFileSync` on components/routes), grandfather the known
  list, convert each via pair-with-positive or rewrite-ticket. Scoped
  to source-reading files so legitimate behavioral negatives stay
  green.
- StrykerJS mutation audit on the frontend suite (periodic, not a PR
  gate): delete/change production code, confirm the suite fails.
  Catches vacuous tests the meta-test pattern misses.

Checks per change: `just frontend-check`, frontend vitest run,
`just plan-lint`. No step claims enforcement until its check runs green
on a deliberate violation (probe pattern from the spike).
