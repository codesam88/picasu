import { test as base } from '@playwright/test'
import { createPaths, type WorkerPaths } from './paths'

export interface ScenarioFixtures {
  backendPaths: WorkerPaths
}

export const test = base.extend<ScenarioFixtures>({
  backendPaths: [
    async ({ browser: _browser }, use) => {
      void _browser
      const paths = await createPaths()
      // Paths only: the backend is started by the test body (interpreter.spec.ts),
      // so this fixture never launches or stops a process.
      await use(paths)
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
