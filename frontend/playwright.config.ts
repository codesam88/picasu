import { defineConfig } from '@playwright/test'
import * as path from 'path'
import * as os from 'os'
import { TEST_DIR } from './tests/playwright/paths'

const isCI = !!process.env.CI

export default defineConfig({
  testDir: './tests/playwright',
  // Hand-written browser tests are plain `.ts` by convention: only the
  // scenario-DSL runner (`interpreter.spec.ts`) is a spec. Name them
  // explicitly so discovery does not depend on the suffix.
  testMatch: [
    'interpreter.spec.ts',
    'first-admin-registration.ts',
    'login-throttle.ts',
    'invite-registration.ts',
    'album-tree-refresh-after-move.ts'
  ],
  fullyParallel: true,
  forbidOnly: isCI,
  // Debug builds pay ~10s per PBKDF2 verify, so KDF-bound scenarios
  // (notably UI login flows) need minutes, not seconds. Timeouts only
  // bite on genuine failures.
  timeout: 120_000,
  retries: isCI ? 2 : 0,
  workers: undefined,
  outputDir: path.join(TEST_DIR, 'artifacts'),
  reporter: [
    ['list'],
    ['json', { outputFile: path.join(TEST_DIR, 'report.json') }],
    ['html', { outputFolder: path.join(TEST_DIR, 'html-report'), open: 'never' }]
  ],

  // KDF-bound actions (notably UI login: ~10s PBKDF2 verify in debug
  // builds) legitimately exceed fast defaults; timeouts only bite on
  // genuine failures, so parity here costs passing runs nothing.
  expect: {
    timeout: 15_000
  },

  use: {
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    launchOptions: {
      executablePath: isCI
        ? undefined
        : path.join(
            os.homedir(),
            '.cache',
            'ms-playwright',
            'chromium-1228',
            'chrome-linux64',
            'chrome'
          ),
      env: {
        ...process.env,
        LD_LIBRARY_PATH: '/tmp/nss-libs/usr/lib/x86_64-linux-gnu'
      }
    }
  }
})
