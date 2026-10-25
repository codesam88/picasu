---
status: open
type: feature
priority: high
area: frontend
---

## Notes

Hand-written Playwright files (plain `.ts`, not specs: `first-admin-registration.ts`,
`login-throttle.ts`, `invite-registration.ts`) bypass the YAML
scenario pipeline, contradicting `docs/playwright_generator.md` ("Rather
than writing Playwright tests by hand ... this project drives them from
YAML scenario files") and the single-pipeline description in
`docs/test-strategy.md`. The general design (DSL verbs) must come first;
individual cases are then declarations in it, not bespoke code that gets
adjusted until green.

## Naming rule

`*.spec.ts` is the executable spec machinery: the scenario YAML files
plus `interpreter.spec.ts`, which runs them. Hand-written browser tests
are tests, not specs, and must be named `*.test.ts` so the suffix never
implies equal standing with the DSL pipeline. The long-term direction is
zero hand-written browser files.

## Implementation steps

1. Re-read `docs/test-strategy.md`, `docs/playwright_generator.md`,
   `docs/scenario-dsl.md` and derive the verb gaps from them, not from
   existing hand-written files: invite creation/redeem (`invite:` given
   with bound/open variants, exposing the token as a `$var`), second
   (non-admin) user seeding plus viewer-session login, API asserts under
   a chosen session (anonymous vs viewer vs admin), and whatever
   `login-throttle` needs (custom backend env, response observation).
2. Extend the DSL: `types.ts` Zod schemas, `schema.json` copy,
   `executeGiven` (invite minting, viewer seeding), interpreter verbs,
   `docs/scenario-dsl.md` reference. One verb at a time, each with a
   YAML scenario that fails before the verb exists.
3. Port the three hand-written flows to YAML scenarios; delete the
   hand-written files once their scenarios pass. Rename any remaining
   hand-written browser file to `*.test.ts`.
4. Update `docs/test-strategy.md` only after the port: single-track
   statement holds again, no second track to document.

## Out of scope

Backend behavior changes. New product features. The viewer denial-matrix
coverage from `e2e-viewer-invite-coverage.md`, which becomes YAML
scenarios under this plan once the needed verbs exist.
