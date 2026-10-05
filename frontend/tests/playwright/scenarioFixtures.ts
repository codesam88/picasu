import { test as base } from '@playwright/test'
import { createPaths, type WorkerPaths } from './paths'
import { startBackend } from './backendLauncher'

export interface ScenarioFixtures {
  backendPaths: WorkerPaths
}

export const test = base.extend<ScenarioFixtures>({
  backendPaths: [
    async ({ browser: _browser }, use) => {
      void _browser
      const paths = await createPaths()
      const handle = await startBackend(paths)
      // `use` does not reject when the test itself fails — the fixture body
      // runs to the end either way — but it does reject if a sibling fixture
      // or the runner errors, and the backend must not outlive that.
      try {
        await use(paths)
      } finally {
        await handle.stop()
      }
    },
    { scope: 'test' }
  ],

  page: [
    async ({ browser, backendPaths }, use) => {
      const context = await browser.newContext({ baseURL: backendPaths.BACKEND_URL })
      const page = await context.newPage()
      await use(page)
      await context.close()
    },
    { scope: 'test' }
  ]
})
