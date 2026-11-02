# Brief: frontend testing with Vitest and Vue

Researched: 2026-10-30 | Versions: vitest 5.0.5, Vue 3, Vuetify 3, @vue/test-utils v2, Node >= 22 | Overall confidence: high on harness setup and tier choice (primary docs + official exemplar agree); medium on browser-mode API details (newest claims read on unreleased `main` docs — pin against 5.0.5 before use)

## 1. Baseline

The standard picture for testing Vue components has three tiers. Unit tests in Vitest assert the
contract: slots render in the right containers, props map to
visibility and layout, and user actions produce the documented events.
They are authored with `mount()` from `@vue/test-utils` v2, a fresh
`createVuetify()` per test (a shared instance leaks display/theme
state), and queries against roles and `data-testid` attributes rather
than framework markup. Teleported content (e.g. dialog bodies) does not
appear in `wrapper.html()`; tests reach it through `getComponent` /
`findComponent` or assert against `document.body`. Mixed suites (pure
logic plus DOM tests) split by per-file `// @vitest-environment`
comments or by test projects. End-to-end tests keep a single smoke per
flow; everything else drops a tier.

Common prior beliefs that research corrects: that `jsdom` is the
default home for Vuetify component tests (the framework docs recommend
a real browser instead [vuetify]); that `environmentMatchGlobs` can
scope environments per file (removed — use projects or per-file
comments [environment][qaskills]); and that teleport and transition
handling needs bespoke mocks (both frameworks document settled
conventions [vuetify][teleport]). Vitest 5 also changed daily-test
mechanics: `clearMocks: true` by default, strict/exact locators,
unawaited async assertions fail, and the `workspace` key is now
`projects` [vitest-5][projects].

## 2. What research adds

### Tier decision: scan, unit, browser component, e2e — assign each behavior once

- **Keep adoption enforcement as a lint tier, separate from behavior tests.** A structural scan (e.g. asserting page templates render through a shared wrapper) proves authors _use_ the wrapper, not that it behaves. This is a deliberate exception to the "test the contract, not internals" rule [practice]; keep it narrow and labeled, and don't extend it to behavior. _Applies when: enforcing adoption of shared structure._
- **Put component behavior in browser-mode component tests, not jsdom.** Vuetify's docs explicitly recommend a real browser over jsdom for Vuetify components and name vitest browser mode, Playwright, and Cypress as the runners [vuetify]. Rationale: dialog behavior (focus trap, Escape, overlay dismiss) and page layout depend on the platform pieces emulators fake. _Applies when: testing anything the user can see or tab to, in apps using component libraries with browser-dependent behavior._
- **Reserve jsdom for logic-adjacent component units** (slot→DOM mapping, conditional rendering) and keep pure logic in `node` env. Neither emulator proves layout, CSS visibility, native focus order, or navigation [environment][qaskills]. _Applies when: the assertion needs no geometry or native events._
- **Do NOT duplicate e2e coverage.** Keep one end-to-end smoke per flow (e.g. page renders, modal opens/closes); everything else drops a tier. _Applies when: tempted to assert wrapper internals in e2e._

### Harness A (recommended): vitest-browser-vue

- **Render with `render()` from `vitest-browser-vue`, passing VTU mount options** (all supported except `attachTo` — use `container` instead) [browser-vue]. Rationale: same authoring model as VTU, but real Chromium/Playwright underneath, so Vuetify behaves like production.
- **Query from `baseElement` (default `document.body`), never `container.querySelector`.** The docs mark direct container queries as a resilience smell and point at locators instead [browser-vue]. Rationale: teleported dialog content lives outside the mount container; body-scoped locators cover portals for free. _Applies when: asserting on a dialog's open state._
- **Prefer content assertions over `emitted()`.** The API exposes `emitted()` but flags it as implementation detail; assert the displayed result of the emission via locators [browser-vue]. Rationale: survives event-name refactors. _Applies when: testing close/submit flows — assert the dialog closed, not that `update:modelValue` fired._
- **Use auto-retrying assertions and role locators**: `await expect.element(screen.getByRole('dialog'))`, `userEvent` for clicks/keys [component]. Rationale: retries absorb async render without manual waits. _Applies when: any async or transitioned UI._
- **Cover the accessibility contract of dialog components**: role, `aria-modal`, focus-on-open, focus trap, `Escape` close — the docs' Modal example is the template [component]. Rationale: this is the behavior a scan can never prove. _Applies when: writing the first browser test for a modal component._

### Harness B (fallback): jsdom + @vue/test-utils (e.g. for Vuetify apps without browser CI)

- **Wire the documented Vuetify setup exactly**: `server.deps.inline: ['vuetify']` in config, install `resize-observer-polyfill` and assign `global.ResizeObserver` in setup, fresh `createVuetify({ components, directives })` per test file (or per test), `global: { plugins: [vuetify], stubs: { transition: false, 'transition-group': false } }` [vuetify]. Rationale: this is the framework author's own recipe — "no need to mock transitions or other internals."
- **Handle teleport explicitly**: teleported content is absent from `wrapper.html()`; reach it with `getComponent`/`findComponent` (VDOM-level), or stub with `global.stubs: { teleport: true }` when testing the rest of the component; create the teleport target in `beforeEach` and clear `document.body` in `afterEach` [teleport]. For `v-dialog`-style teleported dialogs, expect content in `document.body` and assert against the document, not the wrapper [so-dialog: lead]. _Applies when: dialog tests run under jsdom._
- **Select env per file with `// @vitest-environment jsdom`**, or split suites with test projects; do NOT use `environmentMatchGlobs` (removed) [environment][projects][qaskills]. _Applies when: most of the suite stays `node` and only component tests need DOM._
- **Do NOT paste inert global stubs to force green.** A no-op `ResizeObserver` changes the contract (callbacks never run); instead inject a deterministic adapter and test the decision as pure logic, or move the case to a browser [qaskills: lead]. _Applies when: an env-switch failure tempts a quick stub._

### Suite mechanics (Vitest 5)

- **Mix suites with `test.projects`**: inline `{ test: { name, include, environment } }` entries inherit root config by default (`extends: true` is the v5 default); filter runs with `vitest -p <name>`; use `defineProject` in project files; root config is not itself a project unless listed [projects]. Rationale: node unit + browser component suites in one run without per-file comments everywhere. _Applies when: adding the first browser test file._
- **Rely on `clearMocks: true` (new default)** to stop call-history leaking between tests, but still construct fresh stores/Vuetify instances per test for state isolation [vitest-5]. _Applies when: diagnosing order-dependent failures._
- **Await every async assertion and expect strict locators**: unawaited `resolves`/`rejects` now fail, and `getByText` is exact by default [vitest-5]. Rationale: the two most common "passes locally, fails in CI" sources are now compile-time-shaped errors. _Applies when: writing or reviewing any browser test._
- **Hunt flakes with `--repeats`** and collect reports under the single `.vitest/` directory (one `.gitignore` entry) [vitest-5]. _Applies when: a new browser test flakes in CI._

### Mocking and types

- **Never mock the component under test**; mock at the store/router boundary, prefer real pure deps, and use MSW for HTTP [practice]. _Applies when: the component needs store or route data — provide a real store double, not a framework stub._
- **Add `*.test-d.ts` type tests** (`expectTypeOf`/`assertType`, `--typecheck`, tsc/vue-tsc under the hood) for component prop/emit shapes where types carry invariants runtime tests can't pin [types]. _Applies when: props gain conditional types a runtime test can't pin._

### Enforcing structure beyond tests

Vitest proves behavior; bans on structure belong in ESLint, which fails
at authoring time and in the IDE instead of at test time:

- **Ban bypass elements with the restriction rules, not with tests.** `vue/no-restricted-component-names` forbids named components (e.g. a raw dialog where a modal wrapper exists); `vue/no-restricted-html-elements` does the same for native elements; `vue/no-restricted-block` restricts whole SFC blocks. Note the trap: `vue/no-restricted-syntax` targets JavaScript _expressions_ inside `<template>`, not element usage — pick the rule by target [eslint-plugin-vue]. Rationale: same signal as a structural scan, earlier feedback. _Applies when: a wrapper exists and raw usage must fail the lint gate._
- **Ban bare text with `vue/no-bare-strings-in-template`** (and `vue/no-literals-in-template` for literals). Text embedded directly in a page fails unless routed through the i18n function — one rule covering both translatability and the "no naked copy in pages" discipline [eslint-plugin-vue]. Rationale: structural consistency plus i18n for free. _Applies when: copy must live in locale files or designated title components._
- **Take the free conventions first**: `vue/attributes-order`, `vue/require-explicit-emits`, `vue/require-explicit-slots`, component-name casing, `vue/match-component-file-name`, `vue/max-template-depth` — consistent APIs with zero custom code [eslint-plugin-vue]. _Applies when: standardizing any component library._
- **Enforce layering with `eslint-plugin-boundaries`.** Classify files into elements (e.g. pages, shared wrappers), then write allow/disallow policies on `boundaries/dependencies` with a `default: disallow` baseline — pages may compose wrappers, never each other or primitives [boundaries]. Rationale: makes "text and icons embedded directly in a page" a dependency violation, not a review comment. _Applies when: the folder layout implies layers the team keeps violating._
- **Enforce look-and-feel with tokens** (e.g. stylelint rules restricting raw color/spacing values to theme tokens) so theming stays a token change (lead — needs a source read before depending on it). **Cover rendered output with visual regression** (screenshot comparison in the browser runner of choice; the framework docs name Playwright and Cypress as the e2e-grade runners [vuetify]). _Applies when: consistency must survive the gap between markup and pixels._

### Alternatives and when NOT to use the recommended path

- **`@testing-library/vue` instead of `vitest-browser-vue`** when the team prefers its API or is migrating existing tests; bridge via `page.elementLocator()`. Loses auto-retry integration with Vitest locators [component]. _Use when: familiarity outweighs the retry benefit._
- **jsdom-only (no browser project)** when CI cannot run browsers or the team wants zero new infra: acceptable for slot/prop mapping, insufficient for focus/overlay/layout behavior — say so in the test file [vuetify]. _Use when: bootstrapping before browser CI exists._
- **Playwright component testing (experimental)** as an alternative browser runner for React/Vue/Svelte if the team consolidates on Playwright tooling [search]. _Use when: e2e and component tests should share one runner._

### Pitfalls (tagged)

- Teleport assertions against `wrapper.html()` silently pass-vacuous — always assert against the document/body in jsdom, or body-scoped locators in browser mode [teleport][browser-vue] (verified).
- Shared Vuetify singleton leaks display/theme state across tests — fresh instance per test [baseline specialist instinct, confirmed by per-call `render` helper][vuetify] (verified).
- `environmentMatchGlobs` in copied examples is dead config — tests silently run in `node` [qaskills + v5 docs omission] (verified).
- `happyDOM` vs `happy-dom` key confusion in `environmentOptions`; pin explicit `url` origins and avoid production domains [qaskills] (lead).
- `emitted()`-only close tests pass while the dialog stays visibly open — pair with a visibility/content assertion [browser-vue] (verified).

## 3. Worker checklist

Do:

- [ ] Add a `browser` test project (name it, e.g. `component`) with Playwright provider before writing component behavior tests [projects]
- [ ] Write the first modal-dialog browser test: open → role/focus/`aria-modal` → `Escape`/close-button → closed-content assertion [component]
- [ ] Write the first layout-component browser test: slots render in the right containers; loading/empty states [component]
- [ ] Pin `vitest-browser-vue` version against vitest 5.0.5; re-check `attachTo`→`container` and `config` export against installed docs, not `main` [browser-vue]
- [ ] Keep adoption enforcement (e.g. a structural scan) as a separate lint tier; don't extend it to behavior
- [ ] Run the repo's typecheck, lint, and format gates plus the full `vitest run` before claiming done

Don't:

- [ ] Don't assert Vuetify class names, markup, or `emitted()` payloads alone
- [ ] Don't add `environmentMatchGlobs`, inert `ResizeObserver`/geometry stubs, or `container.querySelector` chains
- [ ] Don't mock the component under test; mock stores/router at the boundary
- [ ] Don't copy jsdom examples for dialog/focus tests without escalating to the browser project

Validate: a new browser test fails on a component regression (e.g. remove `aria-modal`, break `Escape` handling) and passes otherwise; each project green when filtered (e.g. `vitest -p unit`, `vitest -p component` once projects with those names exist); no new per-file env comments outside the component project.

## 4. Sources

Primary (depend on these):

- Vuetify unit-testing source (vuetifyjs/vuetify repo docs) — the framework-author recipe: inline dep, ResizeObserver polyfill, per-call `render` helper, transition stubs, real-browser recommendation, `data-testid` [vuetify]
- Vitest Test Environment guide (v5.0.5) — node default, per-file control comments, `server.deps.inline` chain rule, browser-is-a-project [environment]
- Vitest Test Projects guide (v5.0.5) — inline projects, `extends` inheritance, `defineProject`, `-p` filtering [projects]
- Vue Test Utils Teleport guide — `getComponent`/`findComponent`, target setup/teardown, `teleport: true` stub [teleport]
- `vitest-browser-vue` API (main branch) — `render`/`container`/`baseElement`/`emitted`/`config`; re-pin to 5.0.5 [browser-vue]
- Vitest Component Testing guide — browser-mode rationale, auto-retry assertions, accessible-Modal pattern [component]
- Vitest 5.0 announcement — `clearMocks` default, strict locators, unawaited-assertion failures, `--repeats`, `.vitest/` dir [vitest-5]
- Vitest Testing in Practice — contract-first, one-behavior, fresh-state, reproducer-first, MSW [practice]
- `eslint-plugin-vue` rule docs — restriction rules target elements vs expressions; bare-string/literal rules; free convention rules [eslint-plugin-vue]
- `eslint-plugin-boundaries` README + docs — element classification, allow/disallow policies, `default: disallow` [boundaries]
- Vitest Testing Types — `*.test-d.ts`, `expectTypeOf`/`assertType`, `--typecheck` [types]

Secondary (leads only):

- QASkills jsdom-vs-happy-dom (2026-08-08) — `environmentMatchGlobs` removal, `happyDOM` key, inert-stub warning, dual-project contract matrix [qaskills]
- SO 74976651 snippet (unread, 403) — v-dialog content lands in `document.body` under `mount()` [so-dialog]
