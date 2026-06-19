import { test } from '@playwright/test'
import { loadAllScenarios } from './loadScenarios'
import { executeGiven, createGivenContext } from './executeGiven'
import { executeWhen, executeThen } from './interpreter'

const scenarios = loadAllScenarios()

test.describe('UI scenarios', () => {
  for (const scenario of scenarios) {
    test(scenario.name, async ({ page, request }) => {
      const ctx = createGivenContext(scenario.name)
      const seeded = await executeGiven(request, scenario.given, ctx)
      await executeWhen(page, scenario.when, seeded)
      await executeThen(page, scenario.then, seeded)
    })
  }
})
