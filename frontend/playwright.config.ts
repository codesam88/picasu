import { defineConfig } from '@playwright/test'
import * as fs from 'fs'
import * as path from 'path'
import * as os from 'os'
import { TEST_DIR } from './tests/playwright/paths'

const isCI = !!process.env.CI

// Local runs pin the downloaded Chromium (plus the LD_LIBRARY_PATH below),
// but the revision directory (`chromium-NNNN`) changes with every Playwright
// bump. Resolve whatever is actually installed instead of hardcoding it;
// fall back to Playwright's default resolution when the cache is empty.
function localChromiumExecutable(): string | undefined {
  try {
    const cacheDir = path.join(os.homedir(), '.cache', 'ms-playwright')
    const entries = fs
      .readdirSync(cacheDir)
      .filter((e) => /^chromium-\d+$/.test(e))
      .sort()
      .reverse()
    for (const entry of entries) {
      const exe = path.join(cacheDir, entry, 'chrome-linux64', 'chrome')
      if (fs.existsSync(exe)) return exe
    }
  } catch {
    // No readable browser cache — fall through to default resolution.
  }
  return undefined
}

export default defineConfig({
  testDir: './tests/playwright',
  // Only the scenario-DSL runner (`interpreter.spec.ts`) is spec machinery.
  // Hand-written browser files (naming rule: never `*.spec.ts`) run only
  // when named `*.test.ts`, so discovery can never pick up stray `.ts`.
  testMatch: ['interpreter.spec.ts', '*.test.ts'],
  fullyParallel: true,
  forbidOnly: isCI,
  // Default per-test timeout. The E2E harness sets PICASU_WEAK_PWHASH=1,
  // so KDF logins are fast; the yaml-DSL scenarios use a tighter computed
  // budget instead (see interpreter.spec.ts). This default only binds
  // non-DSL test files. Timeouts only bite on genuine failures.
  timeout: 120_000,
  // Overall cap as a hang-backstop only; per-test timeouts do the real
  // enforcement (120s config default, tighter computed budgets for DSL
  // scenarios). Sized generously so a full parallel suite plus CI retries
  // never legitimately trips it; a run that hits this is stuck, not slow.
  globalTimeout: 20 * 60 * 1000,
  retries: isCI ? 2 : 0,
  workers: undefined,
  outputDir: path.join(TEST_DIR, 'artifacts'),
  reporter: [
    ['list'],
    ['json', { outputFile: path.join(TEST_DIR, 'report.json') }],
    ['html', { outputFolder: path.join(TEST_DIR, 'html-report'), open: 'never' }]
  ],

  // Fallback expect timeout where the interpreter passes no explicit
  // tier. Most assertions carry SMALL/LARGE_TIMEOUT_MS explicitly; this
  // only binds the remainder. The harness sets PICASU_WEAK_PWHASH=1, so
  // even KDF-bound actions stay far below it; timeouts only bite on
  // genuine failures, so parity here costs passing runs nothing.
  expect: {
    timeout: 15_000
  },

  use: {
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    launchOptions: {
      executablePath: isCI ? undefined : localChromiumExecutable(),
      env: {
        ...process.env,
        LD_LIBRARY_PATH: '/tmp/nss-libs/usr/lib/x86_64-linux-gnu'
      }
    }
  }
})
