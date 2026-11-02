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

### F9-1 — Route assertions on direct navigations (done 2026-11-02)

For each direct navigation to another user's album (bob leg and
admin leg): first verify what the app actually does (stays, bounces
to login, renders empty) and assert that exact route alongside the
existing content assertions. Done: route assertions added on all
direct navigations; full personal-albums e2e set green
(`9c48d38b`). Review also fixed an interpolation bug the new
assertions stepped around at first (`/album/${$alice_album}` never
interpolates — the interpreter only substitutes single-`$` names —
so both forbidden-nav steps were loading a literal garbage URL; now
`${alice_album}`, with the asserted route naming the real id).

### F9-2 — Real toggle on/off assertions (BLOCKED on

`.plan/bug-personal-timeline-toggle.md`)

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

## F9-0 Inventory (appended)

Branch verified: `feature/personal-user-albums` (git branch --show-current)

Scenario files present:

- `frontend/tests/playwright/scenarios/personal-user-albums.yaml` (73 lines)
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` (308 lines)
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` (308 lines)
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` (184 lines)

Interpreter/assertion handlers verified (`frontend/tests/playwright/interpreter.ts`):

- `ui.visible` via `resolveLocator`, `expect(...).toBeVisible({ timeout: SMALL_TIMEOUT_MS })`
- `ui.hidden` via `expect(...).not.toBeVisible()`
- `ui.route` via `expect(page).toHaveURL(new RegExp(...), { timeout: LARGE_TIMEOUT_MS })` (auto-waiting URL match)
- `ui.text_visible` via `page.getByText(...).first()` with `toBeVisible({ timeout: SMALL_TIMEOUT_MS })` (auto-retrying)
- `ui.text_hidden` via `page.getByText(...).first()` with `not.toBeVisible()`
- `ui.modal` checks `getByRole('dialog')` open/closed
- `ui.toast` checks `getByRole('status').or(page.locator('.v-snackbar'))` visible + contains
- Direct navigation: `when.navigate` does `page.goto(interpolate(...), { timeout: LARGE_TIMEOUT_MS })` with no automatic route assertion.
- Single-spec run: `just frontend-playwright filter="..."` builds if needed, sets `PICASU_BINARY` (debug build), computes workers, runs `cd frontend && npx playwright test --workers="$workers" --grep "{{filter}}"` from frontend dir.

### (a) `wait.ms`-only steps (WHEN has only `wait.ms`, ASSERT checks)

- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 3: `WHEN: [{wait.ms: 500}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 4: `WHEN: [{wait.ms: 1000}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 6: `WHEN: [{wait.ms: 500}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 7: `WHEN: [{wait.ms: 500}]` `ASSERT: [{ui.visible: main/}]`

### (b) Direct navigations with no `ui.route` assertion

- `frontend/tests/playwright/scenarios/personal-user-albums.yaml` STEP 1: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.text_visible: Public Album}, {ui.text_hidden: Viewer}]`
- `frontend/tests/playwright/scenarios/personal-user-albums.yaml` STEP 2: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.text_visible: Admin}, {ui.text_hidden: Viewer}]`
- `frontend/tests/playwright/scenarios/personal-user-albums.yaml` STEP 3: `WHEN: [{navigate: /}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums.yaml` STEP 7: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.text_visible: Public Album}, {ui.text_hidden: Viewer}, {ui.text_hidden: Admin}]`
- `frontend/tests/playwright/scenarios/personal-user-albums.yaml` STEP 8: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.text_visible: Viewer}, {ui.text_hidden: Admin}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 1: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.text_visible: Alice}, {ui.text_hidden: Admin}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 4: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 11: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 18: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 21: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 28: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 30: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 37: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 39: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 41: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-user.yaml` STEP 48: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 1: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 4: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 11: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 18: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 21: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 28: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 30: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 37: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 39: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 41: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-allowed-flow-admin.yaml` STEP 48: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 5: `WHEN: [{navigate: /timeline}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 8: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.text_visible: Alice}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 13: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.text_visible: Bob}, {ui.text_hidden: Alice}, {ui.text_hidden: Admin}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 14: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.text_hidden: Alice}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 15: `WHEN: [{navigate: /album/${$alice_album}}]` `ASSERT: [{ui.text_hidden: alice-secret.jpg}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 16: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.visible: main/}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 20: `WHEN: [{navigate: /my-albums}]` `ASSERT: [{ui.text_visible: Admin}, {ui.text_hidden: Alice}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 21: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.text_hidden: Alice}]`
- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 22: `WHEN: [{navigate: /album/${$alice_album}}]` `ASSERT: [{ui.text_hidden: alice-secret.jpg}]`

### (c) Negative-only assertions lacking a neighboring owner-can-see positive control (as catalogued)

- Negative-only (`ui.text_hidden` present, no `ui.text_visible` in same step):
  - `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 14: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.text_hidden: Alice}]`
  - `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 15: `WHEN: [{navigate: /album/${$alice_album}}]` `ASSERT: [{ui.text_hidden: alice-secret.jpg}]`
  - `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 21: `WHEN: [{navigate: /albums}]` `ASSERT: [{ui.text_hidden: Alice}]`
  - `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` STEP 22: `WHEN: [{navigate: /album/${$alice_album}}]` `ASSERT: [{ui.text_hidden: alice-secret.jpg}]`
- Steps with both (have neighboring positive control in same step): see full list above (e.g. base scenario steps have both visible+hidden).

### (d) Assignment skip comment with surrounding context

- `frontend/tests/playwright/scenarios/personal-user-albums-isolation-flow.yaml` lines ~58-62:

```yaml
# Skip direct assignment UI flow if UI behaves differently; rely on API-level isolation coverage
- when:
    - wait.ms: 1000
  assert:
    - ui.visible: main/
```

(Context: appears after uploading alice-secret.jpg and clicking Upload with toast/modal closed, followed by another wait.ms+visible step; this is the "assignment skip comment" noted in the plan.)

No scenario edits made in this step. No files modified except appending to `.plan/f9-e2e-honesty.md` as requested.
