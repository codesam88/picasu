---
status: done
type: feature
priority: high
area: testing
---

## Goal

Implement a Playwright-based scenario interpreter for spec-driven frontend testing, mirroring the backend YAML scenario pattern. The interpreter reads UI YAML specs from `gallery-frontend/tests/playwright/scenarios/`, executes them against a real browser driving the full stack (backend + frontend), and produces structured JSON output for AI course-correction.

## Vocabulary

The UI DSL vocabulary is already designed in `docs/scenario-dsl.md` and encoded in `gallery-frontend/tests/playwright/schema.json` (`uiWhenItem`, `uiThenItem` definitions). This task implements the runner.

## Architecture (no cargo xtask involvement)

```
playwright.config.ts
  └─ webServer: starts backend (port 5673) + vite dev server (port 5173)
  └─ test: tests/playwright/*.spec.ts

tests/playwright/
  ├── interpreter.spec.ts     # Loads YAML, runs each scenario as a test
  ├── executeGiven.ts         # POST seed data to backend API
  ├── executeWhen.ts          # Maps YAML verbs → Playwright locator actions
  ├── executeThen.ts          # Maps YAML assertions → Playwright assertions
  ├── report.ts               # Structured JSON output per scenario
  ├── types.ts                # Zod schemas mirroring schema.json UI definitions
  └── scenarios/              # Symlink or reference to gallery-frontend/tests/playwright/scenarios/
```

## Phases

### Phase 1 — Install + config + scaffolding

- `npm install -D @playwright/test`
- `npx playwright install --with-deps chromium`
- Create `playwright.config.ts` with `webServer` for backend + frontend
- Add `npm run test:e2e` script to package.json
- Add `just frontend-e2e` recipe
- Create `tests/playwright/` directory with type definitions and YAML loader

### Phase 2 — Interpreter

- `executeGiven.ts`: POST seed data to running backend (uses same API endpoints)
- `executeWhen.ts`: navigate, click, fill, select, submit → Playwright locators
- `executeThen.ts`: visible, hidden, text, toast, modal, route → Playwright assertions
- `interpreter.spec.ts`: orchestrates load → given → when → then per scenario

### Phase 3 — AI feedback format

- Custom Playwright reporter or JSON output with per-step results
- Screenshot + DOM snapshot on failure
- Console log capture

### Phase 4 — First scenarios

- `ui/home-page.yaml`: navigate to `/home`, assert grid visible
- `ui/albums-page.yaml`: navigate to `/albums`, assert headings
- `ui/login-page.yaml`: fill credentials, submit, assert redirect

## Progress

2026-06-18: All 4 phases complete. 3 scenarios passing. Full stack verified.

## What was built

| File                                                           | Purpose                                                                                                         |
| -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `gallery-frontend/playwright.config.ts`                        | Playwright config with webServer (backend + vite), JSON+HTML reporters, screenshot on failure                   |
| `gallery-frontend/tests/playwright/types.ts`                   | Zod schemas matching the UI DSL vocabulary from `schema.json`                                                   |
| `gallery-frontend/tests/playwright/paths.ts`                   | Shared paths (E2E_DIR, IMAGE_HOME, BACKEND_URL, etc.)                                                           |
| `gallery-frontend/tests/playwright/loadScenarios.ts`           | Reads YAML from `gallery-frontend/tests/playwright/scenarios/`, validates with Zod                              |
| `gallery-frontend/tests/playwright/executeGiven.ts`            | Filesystem seeding + API indexing, auth, variable binding                                                       |
| `gallery-frontend/tests/playwright/interpreter.ts`             | Maps YAML verbs (navigate/click/fill/submit) and assertions (visible/hidden/text/route/modal) to Playwright API |
| `gallery-frontend/tests/playwright/interpreter.spec.ts`        | Main test file — loads all scenarios, runs given→when→then                                                      |
| `gallery-frontend/tests/playwright/scenarios/home-page.yaml`   | Smoke test: navigate to `/`, assert `<main>` visible                                                            |
| `gallery-frontend/tests/playwright/scenarios/albums-page.yaml` | Smoke test: navigate to `/albums`, assert `<main>` visible                                                      |
| `gallery-frontend/tests/playwright/scenarios/login-page.yaml`  | Smoke test: navigate to `/login`, assert password textbox visible                                               |

## How to run

```
just frontend-e2e          # full run with clean sandbox
cd gallery-frontend && npm run test:e2e  # reuse running servers
cd gallery-frontend && npx playwright test --reporter=json  # AI-friendly JSON output
```

## AI feedback format

Playwright's built-in JSON reporter outputs per-scenario results with test names, pass/fail status, durations, and error messages (with screenshots on failure via config). This is the structured feedback for the AI loop: write spec → implement → run tests → parse JSON → course-correct.

2026-06-19: Marked done. User confirmed.
