---
status: open
type: chore
priority: medium
area: frontend
---

## Notes

Test-only hardening of the personal-albums Playwright scenarios. No
production-code change is in scope: if making an assertion honest
surfaces a real bug, file it as a finding and stop — do not weaken
the assertion to green.

## Problem / purpose

The personal-albums e2e scenarios
(`frontend/tests/playwright/scenarios/personal-user-albums*.yaml`,
executed by the step interpreter in
`frontend/tests/playwright/interpreter.ts`) contain steps that pass
whether or not the feature works: bare `wait.ms` sleeps followed by
`ui.visible: main/` (proves only the app did not crash), direct
navigations to another user's album with no `ui.route` assertion (a
bounce to login passes as "hidden everywhere"), and at least one
skipped assignment flow hidden behind a comment. Each behavior under
test must be assigned exactly one honest check.

## Solution strategy

Do not extend the interpreter or invent new assertion types: the
vocabulary needed already exists (`ui.route` via auto-waiting URL
matching, `ui.text_visible` / `ui.text_hidden` via auto-retrying
locators, `ui.modal`, `ui.toast`, direct `navigate`). The work is
using it honestly — content assertions about rendered outcomes,
never sleeps-plus-shell. Rejected: rewriting scenarios as
TypeScript specs (the YAML DSL is the established mechanism);
adding new harness features; weakening assertions or touching
production code to make honest tests pass.

## Implementation steps

Each step is one worker invocation and completes exactly one item
below. No step starts until the previous one is reviewed and
committed.

### F9-0 — Inventory and green baseline

Read the personal-albums scenario files end to end (isolation flow,
allowed flows, and the base file). Append to this plan the
exhaustive list of (a) `wait.ms`-only steps, (b) direct navigations
with no `ui.route` assertion, (c) negative-only assertions with no
neighboring owner-can-see positive control, and (d) the assignment
skip comment with its surrounding context. Then run the target spec
file(s) green as a baseline (`npx playwright test --grep "Personal
albums"` or the repo's equivalent filter) and record the
transcript. No scenario edits in this step.

### F9-1 — Route assertions on direct navigations

For each direct navigation to another user's album (bob leg and
admin leg): first verify what the app actually does (stays, bounces
to login, renders empty) and assert that exact route alongside the
existing content assertions. If actual behavior looks wrong, stop
and file it as a finding instead of asserting around it.

### F9-2 — Real toggle on/off assertions

Drive the timeline's "Include personal items in timeline" switch as
the owning user: assert the personal photo's name appears with the
switch on (`ui.text_visible`) and disappears with it off
(`ui.text_hidden`), letting auto-retrying locators absorb timing.
Convert or delete every wait-then-shell-visible step in these
scenarios; no bare `wait.ms` may remain as an assertion.

### F9-3 — Assignment skip: justify or write it

Either write the UI smoke the skip comment avoids (log in as the
non-owner, attempt assignment through the real dialog, assert the
refusal the user would see) or replace the comment with exact
backend-test pointers that pin assignment isolation, with file and
test names. The comment as written — coverage claimed nowhere in
particular — is not an option.

### F9-4 — Positive-control sweep

For every remaining "must not see" assertion, add the neighboring
"owner can see" assertion in the same run (own album visible in
own MyAlbums, own photo present in own timeline opt-in), so a
broken fixture can never make isolation pass vacuously.

### F9-5 — Full verification and closeout

Target spec file(s) green, then the whole e2e UI suite green (or a
documented subset if wall-clock forces it, with the reason
recorded). Confirm no new `wait.ms` steps and no structural
regressions. Mark this plan `done`.

## Explicitly out of scope

- Production-code changes (findings only, filed separately).
- New interpreter steps or assertion types.
- Touching non-personal-albums scenarios except to prove no
  regression.
