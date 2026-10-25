import * as fs from 'fs'
import * as path from 'path'
import { loadAllScenarios } from './loadScenarios'
import { executeGiven, createGivenContext, resetAuthToken } from './executeGiven'
import { enrollIfNeeded, resolveSessionCreds, uiLogin } from './sessionBoot'
import { executeWhen, executeAssert, executeSteps } from './interpreter'
import { CoverageTracer } from './tracer'
import { test } from './scenarioFixtures'
import { createPaths, type WorkerPaths } from './paths'
import { startBackend } from './backendLauncher'
import { provisionCliUsers } from './cliProvision'
import type { ServerDeclaration } from './types'
import type { BrowserContext } from '@playwright/test'

const scenarios = loadAllScenarios()

/** True for paths the backend serves as JSON API routes (vs. SPA pages and
 * static assets, which also originate from the backend in E2E). */
function isBackendApiPath(pathname: string): boolean {
  return /^\/(get|post|put|delete|object|upload)\b/.test(pathname)
}

/// Map a scenario `server:` declaration onto backend env entries. Keys mirror
/// the real Picasu option names; absent `server:` keeps the harness default
/// (bootstrap open, 1000 iterations).
function serverExtraEnv(server: ServerDeclaration | undefined): Record<string, string | undefined> {
  if (!server) return {}
  const extraEnv: Record<string, string | undefined> = {}
  if (server.bootstrap_admin !== undefined) {
    extraEnv['PICASU_BOOTSTRAP_ADMIN'] = server.bootstrap_admin ? 'true' : undefined
  }
  if (server.pbkdf_iterations !== undefined) {
    extraEnv['PICASU_PBKDF_ITERATIONS'] = String(server.pbkdf_iterations)
  }
  if (server.login_ip_base_ban_secs !== undefined) {
    extraEnv['PICASU_LOGIN_IP_BASE_BAN_SECS'] = String(server.login_ip_base_ban_secs)
  }
  return extraEnv
}

test.describe('UI scenarios', () => {
  for (const scenario of scenarios) {
    test(scenario.name, async ({ page, request, backendPaths, browser }) => {
      resetAuthToken()
      // Phase A: offline CLI provisioning. `cli_user:` entries seed users
      // (any role) before launch, when the server does not hold the store.
      // Requires `server:` (the shared fixture backend already runs) and a
      // closed install (`bootstrap_admin: true` refuses to boot on a
      // non-empty store).
      const cliUsers = (scenario.given ?? []).filter(
        (item): item is { cli_user: { user_id: string; password: string; admin?: boolean } } =>
          'cli_user' in item && item.cli_user !== undefined
      )
      const ownsBackend = scenario.server !== undefined
      if (cliUsers.length > 0 && !ownsBackend) {
        throw new Error(
          `Scenario "${scenario.name}" declares cli_user: without server:: ` +
            `offline provisioning needs its own backend (the fixture backend already holds the store).`
        )
      }
      if (cliUsers.length > 0 && scenario.server?.bootstrap_admin === true) {
        throw new Error(
          `Scenario "${scenario.name}" seeds users offline with bootstrap_admin: true: ` +
            `the server refuses to boot with the flag on a non-empty store. Use bootstrap_admin: false.`
        )
      }
      const given = (scenario.given ?? []).filter(
        (item) => !('cli_user' in item && item.cli_user !== undefined)
      )
      // A `server:` declaration boots a scenario-specific backend with the
      // declared env; otherwise the shared fixture backend (harness default:
      // bootstrap open) serves the scenario. The fixture backend still boots
      // underneath and is torn down by the fixture — accepted S1 cost.
      let activePaths: WorkerPaths = backendPaths
      let activePage = page
      const extraEnv = serverExtraEnv(scenario.server)
      let extraHandle: { stop(): Promise<void> } | null = null
      let extraContext: BrowserContext | null = null
      if (ownsBackend) {
        activePaths = await createPaths()
        if (cliUsers.length > 0) {
          await provisionCliUsers(
            activePaths,
            cliUsers.map((item) => item.cli_user),
            scenario.server?.pbkdf_iterations ?? 1000
          )
        }
        extraHandle = await startBackend(activePaths, { extraEnv })
        extraContext = await browser.newContext({ baseURL: activePaths.BACKEND_URL })
        activePage = await extraContext.newPage()
      }
      try {
        const tracer = new CoverageTracer()
        const ctx = createGivenContext()
        // Harness-managed scenarios (no `session: none`) enroll the resolved
        // account before `given:` so `config:` auth runs against a real user,
        // then log in through the UI so the first step starts authenticated.
        const managed = scenario.session !== 'none'
        if (managed) await enrollIfNeeded(request, scenario, activePaths)
        const seeded = await executeGiven(request, given, ctx, tracer, activePaths)

        // Record backend API calls the browser makes (e.g. POST /upload) so
        // `covers.api` entries exercised from the frontend count as covered.
        activePage.on('request', (req) => {
          const url = new URL(req.url())
          if (url.origin === activePaths.BACKEND_URL && isBackendApiPath(url.pathname)) {
            tracer.recordAPI(req.method(), url.pathname)
          }
        })
        // Failed backend calls the browser itself made (not the `api.response`
        // probes): a viewer 401 here explains a session-clearing bounce, so
        // log them with the page URL at response time.
        activePage.on('response', (res) => {
          let url: URL
          try {
            url = new URL(res.url())
          } catch {
            return // Non-HTTP(S) URLs (blob:, data:) carry no backend traffic.
          }
          if (
            url.origin === activePaths.BACKEND_URL &&
            isBackendApiPath(url.pathname) &&
            res.status() >= 400
          ) {
            console.warn(
              `[api-error] ${res.status()} ${res.request().method()} ${url.pathname} at=${activePage.url()}`
            )
          }
        })

        if (managed) await uiLogin(activePage, resolveSessionCreds(scenario))

        if (scenario.steps) {
          await executeSteps(activePage, scenario.steps, seeded, tracer)
        } else {
          await executeWhen(activePage, scenario.when!, seeded)
          await executeAssert(activePage, scenario.assert!, seeded, tracer)
        }

        const warnings = tracer.compare(scenario.covers)

        const coverageDir = path.join(activePaths.DIR, 'coverage')
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
      } finally {
        if (extraContext) await extraContext.close()
        if (extraHandle) await extraHandle.stop()
      }
    })
  }
})
