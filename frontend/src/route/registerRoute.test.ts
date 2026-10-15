import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, test } from 'vitest'

const routeDir = fileURLToPath(new URL('.', import.meta.url))

function readRoute(file: string): string {
  return readFileSync(join(routeDir, file), 'utf8')
}

function readSrc(relative: string): string {
  return readFileSync(join(routeDir, '..', relative), 'utf8')
}

/**
 * First-user enrollment needs a registration screen the boot probe can
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

  test('routes.ts probes enrollment once and forces register on empty', () => {
    const source = readRoute('routes.ts')
    expect(source).toContain('beforeEach')
    expect(source).toContain('getEnrollmentProbe')
    expect(source).toContain('decideBootDestination')
    expect(source).toMatch(/name:\s*'register'/)
  })

  test('routes.ts shares the cached probe so registration can reset it', () => {
    const page = readFileSync(join(routeDir, '../components/Login/RegisterPage.vue'), 'utf8')
    expect(page).toContain('resetEnrollmentProbe')
    const enrollmentSource = readSrc('script/utils/enrollment.ts')
    expect(enrollmentSource).toMatch(/export const getEnrollmentProbe/)
    expect(enrollmentSource).toMatch(/export const resetEnrollmentProbe/)
  })

  test('routes.ts never redirects share links (capability URLs)', () => {
    const source = readRoute('routes.ts')
    expect(source).toMatch(/to\.name === 'share'/)
    expect(source).toMatch(/shareViewPage/)
  })
})
