---
status: open
type: feature
priority: high
area: testing
---

## Goal

Harden the frontend spec-driven E2E system from scaffold to a reliable implementation. The interpreter must handle every verb in the DSL, schema and types must agree, missing or unknown verbs must fail loudly, and coverage tracing must show which API endpoints and UI elements are exercised. Approach is iterative: fix the interpreter, prove each verb works with minimal scenarios, then build one real user-flow scenario.

## Phases

### Phase 1 — Interpreter correctness

Fix the scaffolding so every defined DSL verb actually works and the schemas agree.

- **Wire missing verbs in `interpreter.ts`:** `select` (when), `ui.toast` and `ui.aria_snapshot` (then). Currently 3 of 7 then-verbs and 1 of 5 when-verbs are dead spec — the interpreter silently skips them.
- **Wire `GivenConfig` in `executeGiven.ts`:** `config.read_only_mode` is validated by Zod/JSON Schema but the handler ignores it.
- **Fix `schema.json` `uiThenToast` nesting:** The JSON Schema defines `type` and `contains` as siblings of `ui.toast` (top-level properties), but the Zod schema nests them inside `ui.toast`. Align JSON Schema to match `types.ts`.
- **Variable interpolation in all locator targets:** `interpolate()` is only applied to `navigate` paths, `fill` values, `contains` text, and `route` patterns. `click`, `select`, `visible`, `hidden`, `text` role/label arguments are passed raw — `${var}` references fail silently. Apply interpolation before `resolveLocator()`.
- **Remove hardcoded `page.goto('/')`:** `interpreter.spec.ts:13` navigates to `/` before every when-block. Remove it — the when-block's own first step controls initial navigation.
- **Remove orphaned `dump.helper.ts`:** Lives in `testDir` and executes as a real test on every run. Delete or move outside the test directory.
- **Runtime step validation:** If a when/then step matches no known verb, throw (do not silently no-op). This prevents scenarios from passing with unexercised assertions.
- **Scenario isolation:** Namespace fixture paths per scenario so sequential runs at the same sandbox don't interfere.

### Phase 2 — Basic verb-proving scenarios

Write one scenario per verb to prove the interpreter handles it correctly. Each scenario is minimal — single given, single when, single then — and tests one thing.

Target list (extendable):

| Verb/assertion          | What it proves                                    |
| ----------------------- | ------------------------------------------------- |
| `navigate + ui.visible` | Smoke: page loads, main element rendered (exists) |
| `navigate + ui.route`   | URL changes reflect navigation                    |
| `fill + ui.text`        | Form input accepts text, text appears in element  |
| `click + ui.hidden`     | Interaction toggles element visibility            |
| `click + ui.modal`      | Click opens/closes a dialog                       |
| `submit + ui.route`     | Form submission changes route                     |
| `ui.toast`              | Toast appears with expected type and message      |
| `ui.aria_snapshot`      | ARIA tree matches committed snapshot              |
| `select`                | Dropdown selection works                          |
| `GivenConfig`           | Read-only mode blocks mutations                   |

Where the verb's effect requires backend state (e.g., `GivenConfig`), use `given` to seed it.

These scenarios serve as the interpreter's self-tests — analogous to the backend's `selftest/` scenarios that verify the assertion machinery catches wrong expectations.

### Phase 3 — First real user-flow scenario

With the interpreter proven, write one scenario that exercises a complete user flow with real backend state:

- **Login flow:** seed clean state → navigate to /login → fill password → submit → assert redirect to home page → verify authenticated UI elements visible
- This exercises `given` (empty state), `navigate`, `fill`, `submit`, `ui.route`, `ui.visible` in sequence — all previously verified individually

Then extend with one album-browsing flow:

- **Album view:** create dir_album + photo in given → navigate to /albums → assert album link visible → click album → assert photo visible in grid
- Exercises `dir_album` + `photo` given verbs, navigation, click, and multi-assertion then-block

### Phase 4 — Spec-declared coverage intent + independent tracer

The scenario itself declares what it intends to exercise. The tracer independently verifies the declaration was fulfilled. This makes coverage a first-class spec property, not just post-hoc observation.

**Scenario-level `covers:` block.**

Add an optional top-level key to the scenario schema:

```yaml
name: Login flow
covers:
  api:
    - POST /post/authenticate
  ui:
    - textbox/Password
    - button/Submit
    - heading/Home
given:
  - empty: true
when:
  - navigate: /login
  - fill: textbox/Password, value: admin
  - submit:
then:
  - ui.route: /
    ui.visible: heading/Home
```

`covers.api` lists HTTP method+path pairs the scenario is expected to exercise (via `executeGiven` calls). `covers.ui` lists role/label pairs the scenario is expected to assert against.

**Independent tracers — not derived from scenario assertions.**

Two passive instruments that observe what actually happens during execution:

- **API call tracer:** wraps the `request.fetch` calls in `executeGiven.ts`. Records method + path + status for every HTTP call made during seeding. Output per-scenario and aggregate JSON.
- **UI assertion tracer:** wraps each `executeThen` assertion. Records the role/label pair and whether it passed. Output same structure.

**Expected-vs-actual comparison.**

After each scenario run, compare:

- `covers.api` entries against actual API calls recorded by the tracer → warn if any expected endpoint was never hit
- `covers.ui` entries against actual assertions recorded by the tracer → warn if any expected element was never asserted

Mismatches produce structured warnings in the output report. A scenario that passes all its `then` assertions but did not actually hit a declared endpoint is flagged — it could be passing for the wrong reasons (e.g., stale state, no-op render).

Reports are advisory (do not fail the run) but designed for the AI course-correction loop: run → parse expected-vs-actual → identify untested paths → update scenarios or flag coverage gaps.

## Orthogonal success metric

The system is credible as spec-driven development when: all DSL verbs are wired and proven by passing scenarios, runtime step validation catches unknown verbs as hard failures, every scenario's `covers:` declaration is truthful (no endpoint declared but never hit), and the tracer findings drive scenario additions rather than remaining unexplored.

## Out of scope

- CI integration (handled in a separate task)
- Bidirectional scenario↔endpoint trace (spec→route inventory matching across all scenarios)
- Auto-generation of coverage manifests from ARIA snapshots
- Hot-reload frontend dev server
