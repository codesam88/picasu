import * as fs from 'fs'
import * as path from 'path'
import { loadAllScenarios } from './loadScenarios'
import { executeGiven, createGivenContext, resetAuthToken } from './executeGiven'
import { executeWhen, executeAssert, executeSteps } from './interpreter'
import { CoverageTracer } from './tracer'
import { test } from './scenarioFixtures'

const scenarios = loadAllScenarios()

test.describe('UI scenarios', () => {
  for (const scenario of scenarios) {
    test(scenario.name, async ({ page, request, backendPaths }) => {
      resetAuthToken()
      const tracer = new CoverageTracer()
      const ctx = createGivenContext()
      const seeded = await executeGiven(request, scenario.given, ctx, tracer, backendPaths)

      if (scenario.steps) {
        await executeSteps(page, scenario.steps, seeded, tracer)
      } else {
        await executeWhen(page, scenario.when!, seeded)
        await executeAssert(page, scenario.assert!, seeded, tracer)
      }

      const warnings = tracer.compare(scenario.covers)

      const coverageDir = path.join(backendPaths.DIR, 'coverage')
      fs.mkdirSync(coverageDir, { recursive: true })
      const slug = scenario.name.toLowerCase().replace(/[^a-z0-9]+/g, '-')
      const report = {
        scenario: scenario.name,
        covers: scenario.covers,
        apiCalls: tracer.apiCalls,
        uiCalls: tracer.uiCalls,
        warnings
      }
      fs.writeFileSync(path.join(coverageDir, `${slug}.json`), JSON.stringify(report, null, 2))

      if (warnings.length > 0) {
        console.warn(`\n[coverage] ${scenario.name}:`)
        for (const w of warnings) {
          const label = w.type === 'missing_api' ? 'API never called' : 'UI never asserted'
          console.warn(`  \u26a0 ${label}: ${w.expected}`)
        }
      }
    })
  }
})
