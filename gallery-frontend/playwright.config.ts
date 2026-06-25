import { defineConfig } from '@playwright/test'
import * as path from 'path'
import * as os from 'os'
import { fileURLToPath } from 'url'
import { TEST_DIR } from './tests/playwright/paths'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const isCI = !!process.env.CI

export default defineConfig({
  testDir: './tests/playwright',
  fullyParallel: true,
  forbidOnly: isCI,
  retries: isCI ? 2 : 0,
  workers: undefined,
  outputDir: path.join(TEST_DIR, 'artifacts'),
  reporter: [
    ['list'],
    ['json', { outputFile: path.join(TEST_DIR, 'report.json') }],
    ['html', { outputFolder: path.join(TEST_DIR, 'html-report'), open: 'never' }]
  ],

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
