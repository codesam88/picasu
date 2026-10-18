import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, test } from 'vitest'

const routeDir = fileURLToPath(new URL('.', import.meta.url))

function readRoute(file: string): string {
  return readFileSync(join(routeDir, file), 'utf8')
}

/**
 * First-user enrollment needs a registration screen the boot guard can
 * force before any album or config page. The router module is
 * browser-bound (createWebHistory), so the wiring is checked as source
 * instead of importing it — same pattern as routes.test.ts.
 */
describe('register route', () => {
  test('registerRoute defines /register', () => {
    expect(existsSync(join(routeDir, 'registerRoute.ts'))).toBe(true)
    const source = readRoute('registerRoute.ts')
    expect(source).toMatch(/path:\s*'\/register'/)
    expect(source).toMatch(/name:\s*'register'/)
  })

  test('routes.ts wires the register route', () => {
    const source = readRoute('routes.ts')
    expect(source).toContain('registerRoute')
    expect(source).toMatch(/register:\s*'Register'/)
  })

  test('RegisterPage has username, password and confirm fields', () => {
    const page = join(routeDir, '../components/Login/RegisterPage.vue')
    expect(existsSync(page)).toBe(true)
    const source = readFileSync(page, 'utf8')
    expect(source).toMatch(/userId/)
    expect(source).toMatch(/confirm/i)
    expect(source).toContain('submitRegistration')
    expect(source).toContain('bootstrapUser')
    expect(source).toContain('login')
    expect(source).toMatch(/409/)
    expect(source).toContain('timeline')
  })

  test('routes.ts never redirects share links (capability URLs)', () => {
    const source = readRoute('routes.ts')
    expect(source).toMatch(/to\.name === 'share'/)
    expect(source).toMatch(/shareViewPage/)
  })

  test('S7 guard allows with profile present before any fetch (fast path)', () => {
    // R31 rewrite candidate: source tripwire, superseded by the
    // bootGuard.test.ts behavior tests. The decision lives in
    // bootGuard.ts; routes.ts only wires the fast-path profile read.
    const source = readRoute('routes.ts')
    expect(source).toContain("useSessionStore('mainId')")
    expect(source).toMatch(/\.profile !== null/)
    expect(source).toMatch(/profilePresent:/)
  })

  test('S7 guard performs no probe and holds no cache', () => {
    // R1 by deletion: the guard pings fresh only when sessionless, so no
    // probe import and no cache/reset language may remain in routes.ts.
    const source = readRoute('routes.ts')
    expect(source).not.toContain('getEnrollmentProbe')
    expect(source).not.toContain('decideBootDestination')
    expect(source).not.toContain('resetEnrollmentProbe')
    expect(source).not.toContain('cachedEnrollmentProbe')
  })

  test('S8 guard sessionless + needs-enrollment redirects to register', () => {
    // R31 rewrite candidate: source tripwire, superseded by the
    // bootGuard.test.ts behavior tests. routes.ts only injects the ping.
    const source = readRoute('routes.ts')
    expect(source).toContain('decideBootRoute')
    expect(source).toContain('serverInfo: getServerInfo')
  })

  test('S9 guard sessionless + enrolled redirects to login', () => {
    // R31 rewrite candidate: source tripwire, superseded by the
    // bootGuard.test.ts behavior tests. The register arm absorbs the
    // deleted RegisterPage leave-redirect: a sessionless /register visit
    // lands on /login instead of rendering a dead enrollment form.
    const source = readRoute('routes.ts')
    expect(source).toContain('decideBootRoute')
  })

  test('S10 guard never redirects share links', () => {
    // R31 rewrite candidate: source tripwire, superseded by the
    // bootGuard.test.ts behavior tests. The share exemption lives in the
    // module; routes.ts only passes isShareTarget, and the ordering pin
    // below checks the module source instead of routes.ts.
    const source = readRoute('routes.ts')
    expect(source).toContain('isShareTarget')
    const guard = readRoute('bootGuard.ts')
    const shareAt = guard.indexOf('deps.isShareTarget')
    const pingAt = guard.indexOf('await deps.serverInfo()')
    expect(shareAt).toBeGreaterThanOrEqual(0)
    expect(pingAt).toBeGreaterThanOrEqual(0)
    expect(shareAt).toBeLessThan(pingAt)
  })
})
