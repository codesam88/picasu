---
status: idea
type: feature
priority: low
area: frontend
---

## Notes

Adopt `vue-i18n` for the frontend so user-facing copy lives in locale
files instead of inline template text. Only worth doing if translations
become a goal — the UI is English-only today.

## Steps

1. Add `vue-i18n`, extract the ~187 bare strings in 53 files (counted
   2026-11-07 via a trial `vue/no-bare-strings-in-template` run) to
   locale files.
2. Enable `vue/no-bare-strings-in-template` at error level once copy
   flows through the i18n function.
3. Decide ongoing locale burden (single maintainer) before committing
   to this.

## Out of scope

Structural copy placement is already enforced by the wrapper bans in
`frontend-structure-rework`; this item is about translatability only.
