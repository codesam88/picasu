import * as fs from 'fs'
import * as path from 'path'
import { loadAllScenarios } from './loadScenarios'
import { executeGiven, createGivenContext, resetAuthToken } from './executeGiven'
import { executeWhen, executeAssert, executeSteps } from './interpreter'
import { CoverageTracer } from './tracer'
import { test } from './scenarioFixtures'
import { startBackend } from './backendLauncher'
import { provisionCliUsers } from './cliProvision'
import type { ServerDeclaration } from './types'

/**
 * Timeout budget guidance: when a scenario times out, do NOT extend the
 * timeout. Split the scenario into smaller flows, shrink fixtures, or fix
 * the stuck step. With cheap KDF enabled in the harness, even logins are fast;
 * local HTTP is fast. A timeout indicates a stuck step or an oversized
 * scenario — full stop except small-fixture indexing.
 */
const TIMEOUT_GUIDANCE =
  '\n\n[TIMEOUT GUIDANCE] Scenario timed out.\n' +
  '- Do NOT extend the timeout. Split this scenario into smaller per-user/per-area flows.\n' +
  '- Shrink fixtures (fewer photos/albums) or fix the stuck step.\n' +
  '- With PICASU_WEAK_PWHASH=1 in the E2E harness, logins are fast and local HTTP is fast. ' +
  'Only small-fixture indexing is a known-slow case; a timeout usually means stuck/oversized.'

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
  if (server.login_ip_base_ban_secs !== undefined) {
    extraEnv['PICASU_LOGIN_IP_BASE_BAN_SECS'] = String(server.login_ip_base_ban_secs)
  }
  return extraEnv
}

function countLogins(scenario: any): number {
  let count = 0
  const steps = scenario.steps as any[] | undefined
  if (steps) {
    for (const s of steps) {
      if (s.when && Array.isArray(s.when)) {
        for (const w of s.when) {
          if (w && typeof w === 'object' && 'login' in w) count++
        }
      }
    }
  }
  if (scenario.when && Array.isArray(scenario.when)) {
    for (const w of scenario.when) {
      if (w && typeof w === 'object' && 'login' in w) count++
    }
  }
  return count
}

function countSteps(scenario: any): number {
  if (Array.isArray(scenario.steps)) {
    return scenario.steps.length
  }
  const whenLen = Array.isArray(scenario.when) ? scenario.when.length : 0
  const assertLen = Array.isArray(scenario.assert) ? scenario.assert.length : 0
  return whenLen + assertLen
}

function scenarioTimeoutMs(scenario: any): number {
  const logins = countLogins(scenario)
  const steps = countSteps(scenario)
  // Formula (evidence-based, with PICASU_WEAK_PWHASH=1): base for startup/teardown
  // + small allowance for logins (fast with weak KDF) + step budget based on SMALL/LARGE tiers.
  // Hard ceiling well below globalTimeout to fail fast on oversized/stuck scenarios.
  const base = 15000
  const loginAllowance = logins * 2000 // fast with cheap KDF
  const stepBudget = steps * 1000
  const computed = base + loginAllowance + stepBudget
  const ceiling = 60000 // well below 120000 global timeout
  return Math.min(Math.max(computed, 15000), ceiling)
}

function withTimeoutGuidance(err: unknown, _scenarioName: string): Error {
  if (err instanceof Error) {
    if (err.message.includes('[TIMEOUT GUIDANCE]')) return err
    const e = new Error(err.message + TIMEOUT_GUIDANCE)
    e.stack = err.stack
    return e
  }
  const e = new Error(String(err) + TIMEOUT_GUIDANCE)
  return e
}

test.describe('UI scenarios', () => {
  for (const scenario of scenarios) {
    test(scenario.name, async ({ page, request, backendPaths }, testInfo) => {
      testInfo.setTimeout(scenarioTimeoutMs(scenario))
      try {
        resetAuthToken()
        // Phase A: offline CLI provisioning. `cli_user:` entries seed users
        // (any role) before launch, when the server does not hold the store.
        // The store must be seeded against a closed install: provisioning
        // against a bootstrap-open default would make the launcher refuse to
        // boot on the non-empty store.
        const cliUsers = (scenario.given ?? []).filter(
          (item): item is { cli_user: { user_id: string; password: string; admin?: boolean } } =>
            'cli_user' in item && item.cli_user !== undefined
        )
        if (cliUsers.length > 0 && scenario.server?.bootstrap_admin !== false) {
          throw new Error(
            `Scenario "${scenario.name}" declares cli_user but not server.bootstrap_admin: false — ` +
              `cli_user requires an explicit closed install: the launcher default (bootstrap open) ` +
              `refuses to boot against a provisioned store, and bootstrap_admin: true refuses too.`
          )
        }
        const given = (scenario.given ?? []).filter(
          (item) => !('cli_user' in item && item.cli_user !== undefined)
        )
        // One backend per scenario, started here on the fixture-provided paths.
        // `server:` maps to env via serverExtraEnv (absent -> launcher defaults).
        const activePaths = backendPaths
        const activePage = page
        if (cliUsers.length > 0) {
          await provisionCliUsers(
            activePaths,
            cliUsers.map((item) => item.cli_user)
          )
        }
        const extraEnv = serverExtraEnv(scenario.server)
        const backendHandle = await startBackend(activePaths, { extraEnv })
        try {
          const tracer = new CoverageTracer()
          const ctx = createGivenContext()
          // Scenarios declare their own session: a `given: user:` bootstrap plus a
          // `login:` step. The harness no longer auto-enrolls or auto-logs-in.
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
            if (url.origin === activePaths.BACKEND_URL) {
              seeded.httpStatuses.push({ pathname: url.pathname, status: res.status() })
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

          // Scenarios declare their own session via a `login:` step; there is
          // no harness-managed session.

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
        } catch (err: unknown) {
          const e = err as Error
          if (e?.name === 'TimeoutError' || /timeout/i.test(e?.message || '')) {
            throw withTimeoutGuidance(err, scenario.name)
          }
          throw err
        } finally {
          await backendHandle.stop()
        }
      } catch (err: unknown) {
        const e = err as Error
        if (e?.name === 'TimeoutError' || /timeout/i.test(e?.message || '')) {
          throw withTimeoutGuidance(err, scenario.name)
        }
        throw err
      }
    })
  }
})
