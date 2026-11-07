# Brief: Vue code-structure enforcement + Vitest testing layers

Researched: 2026-11-06 | Versions: Vitest 5.0.5, Vue 3, Vuetify 4 (verified against 4.3.1), Vue Router 4, Pinia, @vue/test-utils v2, vitest-browser-vue, ESLint 10 + eslint-plugin-vue (flat config), TypeScript + vue-tsc, Node >= 22 | Overall confidence: high on tier assignment and ESLint rule targets (primary docs agree); medium on vitest-browser-vue API details (some claims read from `main` docs — pin against the installed 5.0.5 before depending on them).

## 1. Baseline

Vitest runs in Node by default and offers three DOM-bearing options: `jsdom`, `happy-dom`, and a real **Browser Mode** project (Playwright/WebdriverIO provider). `environmentMatchGlobs` is gone — scope per file with `// @vitest-environment <name>` comments or split suites with `test.projects`. Vitest 5 renames the old `workspace` key to `projects`; inline projects inherit the root config by default (`extends: true` is the v5 default), are filtered with `vitest -p <name>`, and are defined with `defineProject` inside project files; the root config is not itself a project unless listed.

Component tests follow a contract-first shape: props in, events/rendered output out, authored with `mount()` from `@vue/test-utils` v2. Vuetify components need a fresh `createVuetify({ components, directives })` per test (shared instances leak display/theme state) and the framework's documented jsdom recipe: `server.deps.inline: ['vuetify']`, a `resize-observer-polyfill` assigned to `global.ResizeObserver` in setup, and `global: { plugins: [vuetify], stubs: { transition: false, 'transition-group': false } }` — no mocking of transitions or other internals. Vuetify 4's default theme is `system`, so pass `theme: { defaultTheme: 'light' }` to `createVuetify` for deterministic renders. Teleported content (dialog bodies) is absent from `wrapper.html()`; reach it with `getComponent`/`findComponent` (VDOM level) or stub with `teleport: true`, creating the teleport target in `beforeEach` and clearing `document.body` in `afterEach`.

Vuetify's own docs recommend a **real browser** over jsdom for Vuetify components and name Vitest browser mode, Playwright, and Cypress as the runners. In browser mode, `render()` from `vitest-browser-vue` supports all VTU mount options except `attachTo` (use `container`); `baseElement` defaults to `document.body` (or the container) and is the scope for locators; `emitted()` exists but is flagged as an implementation detail — assert the displayed result via locators instead. Auto-retrying assertions (`await expect.element(...)`) and role locators absorb async render without manual waits.

ESLint enforces structure at authoring time: `vue/no-restricted-html-elements` bans specific elements (and component names used as elements in templates, e.g. `RouterLink`); `vue/no-restricted-block` bans whole SFC blocks; `vue/no-restricted-syntax` targets **JavaScript expressions inside `<template>`**, not element usage. `vue/no-bare-strings-in-template` bans bare copy; `vue/no-literals-in-template` bans object/array/function literals in `v-bind` (reference stability). `eslint-plugin-boundaries` classifies files into elements and writes allow/disallow dependency policies with a `default: disallow` baseline.

## 2. What research adds

### Tier assignment — assign each behavior once

- **Lint tier (not tests) for adoption enforcement.** A scan asserting "page templates render through a shared wrapper" proves authors _use_ the wrapper, not that it behaves. Keep it narrow, label it an adoption check, and do not extend it to behavior. _Applies when: enforcing adoption of shared structure; rationale: lint fails earlier (IDE + CI) than tests and needs no runtime._
- **Browser-mode component tests for visible/interactive behavior.** Vuetify explicitly recommends a real browser over jsdom for Vuetify components [vuetify]. Dialog behavior (focus trap, Escape, overlay dismiss) and page layout depend on platform pieces emulators fake. _Applies when: the assertion needs geometry, native focus order, or real events._
- **jsdom for logic-adjacent component units** (slot→DOM mapping, conditional rendering) and **node for pure logic**. Neither emulator proves layout, CSS visibility, native focus order, or navigation [environment]. _Applies when: the assertion needs no geometry or native events._
- **One e2e smoke per flow** (page renders, modal opens/closes); everything else drops a tier. Do not duplicate e2e coverage in component tests [component]. _Applies when: tempted to assert wrapper internals end-to-end._

### ESLint vs. Vitest SFC scan vs. runtime wrappers

| Enforcement goal                                               | Best mechanism                                                                 | Why                                                                         |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| Ban a raw element/component in templates                       | `vue/no-restricted-html-elements` (element names, incl. PascalCase components) | Authoring-time feedback; covers template usage directly                     |
| Ban a whole SFC block                                          | `vue/no-restricted-block`                                                      | Targets `<style>`, custom blocks                                            |
| Ban a JS expression pattern in `<template>`                    | `vue/no-restricted-syntax` with vue-eslint-parser AST selectors                | Expression-level, not element-level — do not misuse for element bans        |
| Ban naked copy in pages                                        | `vue/no-bare-strings-in-template` (+ `no-literals-in-template`)                | i18n + structural consistency in one rule                                   |
| Layering (pages compose wrappers, never each other/primitives) | `eslint-plugin-boundaries` with `default: disallow` policies                   | Dependency direction becomes a lint error, not a review comment             |
| Prove a wrapper is actually used                               | A narrow Vitest SFC scan (adoption check)                                      | Lint cannot prove usage of a runtime wrapper; keep it out of behavior tests |
| Prove a wrapper behaves correctly                              | Browser component tests                                                        | Behavior belongs in tests, not scans                                        |

- **Trap: `vue/no-restricted-component-names` restricts the declared `name` property of a component definition**, not template usage of that component. To ban usage, use `no-restricted-html-elements` with the component name [eslint-plugin-vue]. _Applies when: configuring bans; rationale: picking the wrong rule silently passes._
- **Free conventions first, zero custom code:** `vue/attributes-order`, `vue/require-explicit-emits`, `vue/require-explicit-slots`, component-name casing, `vue/match-component-file-name`, `vue/max-template-depth` [eslint-plugin-vue]. _Applies when: standardizing any component library._

### Vitest 5 mechanics

- **Mix suites with `test.projects`.** Inline `{ test: { name, include, environment } }` entries inherit root config by default (`extends: true` since v5); filter runs with `vitest -p <name>`; use `defineProject` in project files; the root config is not itself a project unless listed [projects]. _Applies when: adding the first browser test file alongside node tests._
- **`clearMocks: true` is the new default** — call history resets between tests, but still construct fresh stores/Vuetify instances per test for state isolation [vitest-5]. _Applies when: diagnosing order-dependent failures._
- **Strict locators:** `getByText` is exact by default; unawaited `resolves`/`rejects` assertions now fail [vitest-5]. _Applies when: writing or reviewing any browser test — the two common "passes locally, fails in CI" sources._
- **Hunt flakes with `--repeats`** and collect reports under the single `.vitest/` directory (one `.gitignore` entry) [vitest-5]. _Applies when: a new browser test flakes in CI._
- **Type tests:** add `*.test-d.ts` files using `expectTypeOf`/`assertType` and run with `--typecheck` (tsc/vue-tsc under the hood) for component prop/emit shapes runtime tests cannot pin [types]. _Applies when: props gain conditional types._

### Browser-mode harness (vitest-browser-vue)

- **Render with `render()` from `vitest-browser-vue`, passing VTU mount options** (all supported except `attachTo` — use `container`) [browser-vue]. _Rationale: same authoring model as VTU, real Chromium/Playwright underneath, so Vuetify behaves like production._
- **Query from `baseElement` (default `document.body`), never `container.querySelector`.** Direct container queries are a resilience smell; use locators [browser-vue]. Teleported dialog content lives outside the mount container, so body-scoped locators cover portals for free. _Applies when: asserting on a dialog's open state._
- **Prefer content assertions over `emitted()`.** The API exposes `emitted()` but flags it as implementation detail; assert the displayed result of the emission via locators [browser-vue]. _Applies when: testing close/submit flows — assert the dialog closed, not that `update:modelValue` fired._
- **Auto-retrying assertions + role locators:** `await expect.element(screen.getByRole('dialog'))`, `userEvent` for clicks/keys [component]. _Applies when: any async or transitioned UI._
- **Modal accessibility contract (first browser test for a modal):** role, `aria-modal`, focus-on-open, focus trap, `Escape` close — the component-testing guide's Modal example is the template [component]. _Rationale: behavior a scan can never prove._
- **Page-template contract (first browser test for a layout wrapper):** named slots render in the right containers; loading/empty/error states; no Vuetify class-name assertions [component].

### jsdom fallback (Harness B)

- **Wire the documented Vuetify recipe exactly:** `server.deps.inline: ['vuetify']`, `resize-observer-polyfill` assigned to `global.ResizeObserver` in setup, fresh `createVuetify({ components, directives })` per test file (or per test), `global: { plugins: [vuetify], stubs: { transition: false, 'transition-group': false } }` [vuetify]. _Rationale: framework author's recipe — "no need to mock transitions or other internals."_
- **Handle teleport explicitly:** teleported content is absent from `wrapper.html()`; reach it with `getComponent`/`findComponent` (VDOM-level), or stub with `global.stubs: { teleport: true }` when testing the rest; create the teleport target in `beforeEach` and clear `document.body` in `afterEach` [teleport]. For `v-dialog`-style teleported dialogs, expect content in `document.body` and assert against the document, not the wrapper [so-dialog: lead]. _Applies when: dialog tests run under jsdom._
- **Select env per file with `// @vitest-environment jsdom`**, or split suites with test projects; do not use `environmentMatchGlobs` (removed) [environment][projects]. _Applies when: most of the suite stays `node` and only component tests need DOM._
- **Do not paste inert global stubs to force green.** A no-op `ResizeObserver` changes the contract (callbacks never run); instead inject a deterministic adapter and test the decision as pure logic, or move the case to a browser [qaskills: lead]. _Applies when: an env-switch failure tempts a quick stub._

### Mocking and dependencies

- **Never mock the component under test**; mock at the store/router boundary, prefer real pure deps, and use MSW for HTTP [practice]. _Applies when: the component needs store or route data — provide a real store double, not a framework stub._
- **Enforce layering with `eslint-plugin-boundaries`.** Classify files into elements (e.g. pages, shared wrappers, primitives), then write allow/disallow policies on `boundaries/dependencies` with a `default: disallow` baseline — pages may compose wrappers, never each other or primitives [boundaries]. _Rationale: makes "text and icons embedded directly in a page" a dependency violation, not a review comment. _Applies when: the folder layout implies layers the team keeps violating._

### Pitfalls (tagged)

- Teleport assertions against `wrapper.html()` silently pass-vacuous — assert against document/body in jsdom, or body-scoped locators in browser mode [teleport][browser-vue] (verified).
- Shared Vuetify singleton leaks display/theme state across tests — fresh instance per test [vuetify] (verified).
- `environmentMatchGlobs` in copied examples is dead config — tests silently run in `node` [environment][qaskills] (verified).
- `happyDOM` vs `happy-dom` key confusion in `environmentOptions`; pin explicit `url` origins and avoid production domains [qaskills] (lead).
- `emitted()`-only close tests pass while the dialog stays visibly open — pair with a visibility/content assertion [browser-vue] (verified).
- `no-restricted-syntax` misused for element bans — it targets template expressions, not element usage; use `no-restricted-html-elements` [eslint-plugin-vue] (verified).
- `no-restricted-component-names` misused for template bans — it targets the declared `name` property [eslint-plugin-vue] (verified).
- Vuetify 4 `useDisplay` breakpoints moved (md 960→840, lg 1280→1145, xl 1920→1545, xxl 2560→2138) — re-baseline any display assertion copied from v3 examples [vuetify] (verified).

### Alternatives and when NOT to use the recommended path

- **`@testing-library/vue` instead of `vitest-browser-vue`** when familiarity outweighs the retry benefit; bridge via `page.elementLocator()`. Loses auto-retry integration with Vitest locators [component]. _Use when: migrating existing Testing Library tests._
- **jsdom-only (no browser project)** when CI cannot run browsers or the team wants zero new infra: acceptable for slot/prop mapping, insufficient for focus/overlay/layout behavior — say so in the test file [vuetify]. _Use when: bootstrapping before browser CI exists._
- **Playwright component testing (experimental)** as an alternative browser runner if the team consolidates on Playwright tooling [browser]. _Use when: e2e and component tests should share one runner._
- **Visual regression** (screenshot comparison) for look-and-feel that markup checks cannot catch; Vuetify names Playwright and Cypress as e2e-grade runners [vuetify]. _Use when: consistency must survive the gap between markup and pixels._

## 3. Worker checklist

Do:

- [ ] Confirm installed Vitest is 5.0.5 and pin `vitest-browser-vue` against it; re-check `attachTo`→`container`, `config` export, and `emitted` semantics against installed docs, not `main`.
- [ ] Confirm the installed Vuetify is 4.x against the v4 testing docs this brief targets (verified against 4.3.1).
- [ ] Add a `browser` test project (name it, e.g. `component`) with Playwright provider before writing component behavior tests [projects].
- [ ] Keep a `unit` node project for pure logic and a `component` browser project; filter runs with `vitest -p unit` / `vitest -p component`.
- [ ] Write the first modal-dialog browser test: open → role/focus/`aria-modal` → `Escape`/close-button → closed-content assertion [component].
- [ ] Write the first page-template browser test: named slots in the right containers; loading/empty/error states; no class-name assertions [component].
- [ ] Configure adoption enforcement as ESLint bans (`no-restricted-html-elements`, `no-restricted-block`, `no-bare-strings-in-template`) plus `eslint-plugin-boundaries` layering; reserve any Vitest SFC scan for proving wrapper _usage_, labeled as an adoption check.
- [ ] Add `*.test-d.ts` type tests for component prop/emit shapes where types carry invariants.
- [ ] Run typecheck, lint, format, and the full `vitest run` before claiming done.

Don't:

- [ ] Don't assert Vuetify class names, markup, or `emitted()` payloads alone.
- [ ] Don't add `environmentMatchGlobs`, inert `ResizeObserver`/geometry stubs, or `container.querySelector` chains.
- [ ] Don't mock the component under test; mock stores/router at the boundary.
- [ ] Don't copy jsdom examples for dialog/focus tests without escalating to the browser project.
- [ ] Don't misuse `no-restricted-syntax` for element bans or `no-restricted-component-names` for template bans.

Validate: a new browser test fails on a component regression (e.g. remove `aria-modal`, break `Escape` handling) and passes otherwise; each project green when filtered (`vitest -p unit`, `vitest -p component`); no new per-file env comments outside the component project; lint fails on a deliberately introduced raw element/page-layer violation.

## 4. Sources

Primary (depend on these):

- Vuetify 4 unit-testing docs (vuetifyjs.com/en/getting-started/unit-testing/) — real-browser recommendation, jsdom recipe (inline dep, ResizeObserver polyfill, per-test `createVuetify`, transition stubs), `data-testid`, Playwright/Cypress runners [vuetify].
- Vitest Browser Mode guide (v5.0.5) — provider/instance config, headless/UI defaults, thread-blocking dialog mocks, spying limits [browser].
- Vitest Component Testing guide (v5.0.5) — contract-first behavior, auto-retry assertions, accessible-Modal example, Testing Library bridge [component].
- Vitest Test Projects guide (v5.0.5) — inline projects, `extends` inheritance, `defineProject`, `-p` filtering, nested projects [projects].
- Vitest Test Environment guide (v5.0.5) — node default, per-file control comments, removed `environmentMatchGlobs`, `server.deps.inline` chain rule, browser-is-a-project [environment].
- Vitest 5.0 announcement — `clearMocks` default, strict/exact locators, unawaited-assertion failures, `--repeats`, `.vitest/` dir [vitest-5].
- Vitest Testing in Practice — contract-first, one-behavior, fresh-state, MSW [practice].
- Vitest Testing Types — `*.test-d.ts`, `expectTypeOf`/`assertType`, `--typecheck` [types].
- `vitest-browser-vue` API (main) — `render`/`container`/`baseElement`/`emitted`/`config`; re-pin to 5.0.5 [browser-vue].
- Vue Test Utils Teleport guide — `getComponent`/`findComponent`, target setup/teardown, `teleport: true` stub [teleport].
- `eslint-plugin-vue` rule docs (raw) — restriction rules' targets (elements vs expressions vs blocks vs declared names), bare-string/literal rules, free convention rules [eslint-plugin-vue].
- `eslint-plugin-boundaries` docs (jsboundaries.dev) — element/file classification, allow/disallow policies, `default: disallow`, selectors [boundaries].

Secondary (leads only):

- QASkills jsdom-vs-happy-dom (2026-08-08) — `environmentMatchGlobs` removal, `happyDOM` key, inert-stub warning, dual-project contract matrix [qaskills].
- SO 74976651 snippet (unread, 403) — v-dialog content lands in `document.body` under `mount()` [so-dialog].
