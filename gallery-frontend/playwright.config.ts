import { defineConfig } from '@playwright/test'
import * as path from 'path'
import * as os from 'os'
import { fileURLToPath } from 'url'
import { CONFIG_DIR, DATA_DIR, IMAGE_HOME, BACKEND_URL } from './tests/playwright/paths'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const isCI = !!process.env.CI

export default defineConfig({
  testDir: './tests/playwright',
  fullyParallel: true,
  forbidOnly: isCI,
  retries: isCI ? 2 : 0,
  workers: isCI ? 1 : undefined,
  reporter: [
    ['list'],
    ['json', { outputFile: 'playwright-report/results.json' }],
    ['html', { open: 'never' }]
  ],

  use: {
    baseURL: BACKEND_URL,
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
  },

  webServer: [
    {
      command: 'cargo run --bin urocissa',
      cwd: path.resolve(__dirname, '..', 'gallery-backend'),
      port: 5673,
      reuseExistingServer: !process.env.CI,
      env: {
        UROCISSA_CONFIG_HOME: CONFIG_DIR,
        UROCISSA_DATA_HOME: DATA_DIR,
        UROCISSA_IMAGE_HOME: IMAGE_HOME
      },
      timeout: 180000
    }
  ]
})
