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
`docs/briefs/vitest-frontend-testing.md`. Prior spike (branch
`spike/vue-structure-enforcement`) inventoried the current state and
prototyped a structural scan.

Starting position: all top-level pages render through `PageTemplate`;
`BaseModal`/`ShareModalBase` exist but most modals still use raw
`v-dialog`; `Page/Config/*` sections hand-roll cards and inline confirm
dialogs.

## Steps

1. Port the spike's structural scan (`designStructure` rule + test) as
   the adoption gate, keeping the grandfather list for known debt.
2. Lint-layer enforcement (earlier feedback than tests):
   `vue/no-restricted-component-names` for raw dialogs/cards where
   wrappers exist, `vue/no-bare-strings-in-template` for naked copy,
   plus free convention rules (`attributes-order`,
   `require-explicit-emits`/`require-explicit-slots`).
3. Migrate the debt: raw-dialog modals to `BaseModal`, inline confirms
   to a shared confirm dialog, Config sections to a shared section
   card; shrink the grandfather list to zero, then delete it.
4. Behavior tests: browser test project (`vitest-browser-vue`) with the
   modal accessibility contract (role, focus, `Escape`) and page
   template slots/loading states first.
5. Later, if the layering keeps being violated: `eslint-plugin-boundaries`
   sketch, theme-token lint, visual regression coverage.

Checks per change: `just frontend-check`, frontend vitest run,
`just plan-lint`. No step claims enforcement until its check runs green
on a deliberate violation (probe pattern from the spike).
