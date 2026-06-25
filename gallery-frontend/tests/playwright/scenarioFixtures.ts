import { test as base } from '@playwright/test'
import { createPaths, type WorkerPaths } from './paths'
import { startBackend } from './backendLauncher'

export type ScenarioFixtures = {
  backendPaths: WorkerPaths
}

export const test = base.extend<ScenarioFixtures>({
  backendPaths: [
    async ({}, use) => {
      const paths = createPaths()
      const handle = await startBackend(paths)
      await use(paths)
      await handle.stop()
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
