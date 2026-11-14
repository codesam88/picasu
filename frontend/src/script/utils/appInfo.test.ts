import { describe, expect, test, beforeEach, afterEach } from 'vitest'
import { APP_VERSION, getAppVersion, collectDiagnostics } from './appInfo'
import { clearErrorLog, installErrorLog } from './errorLog'

describe('appInfo', () => {
  beforeEach(() => {
    clearErrorLog()
  })

  afterEach(() => {
    delete (globalThis as { window?: unknown }).window
    clearErrorLog()
  })

  test('APP_VERSION matches semver', () => {
    expect(APP_VERSION).toMatch(/^\d+\.\d+\.\d+$/)
  })

  test('getAppVersion returns APP_VERSION', () => {
    expect(getAppVersion()).toBe(APP_VERSION)
    expect(getAppVersion()).toMatch(/^\d+\.\d+\.\d+$/)
  })

  test('collectDiagnostics returns exactly the agreed keys', () => {
    const d = collectDiagnostics('/foo')
    expect(Object.keys(d).sort()).toEqual(
      ['appVersion', 'route', 'userAgent', 'timestamp', 'viewport', 'recentErrors'].sort()
    )
  })

  test('appVersion mirrors package and route passthrough', () => {
    const d = collectDiagnostics('/bar')
    expect(d.appVersion).toBe('2.1.0')
    expect(d.route).toBe('/bar')
  })

  test('timestamp is an ISO string', () => {
    const d = collectDiagnostics('/')
    expect(new Date(d.timestamp).toISOString()).toBe(d.timestamp)
  })

  test('recentErrors passthroughs the capped ring buffer', () => {
    const w: {
      onerror: ((...args: unknown[]) => unknown) | null
      addEventListener: (type: string, listener: (event: unknown) => void) => void
    } = {
      onerror: null,
      addEventListener: () => undefined
    }
    ;(globalThis as { window?: unknown }).window = w
    installErrorLog()
    for (let i = 0; i < 25; i++) {
      if (w.onerror !== null) w.onerror(`e${i}`)
    }
    const d = collectDiagnostics('/')
    expect(d.recentErrors).toHaveLength(20)
    expect(d.recentErrors[0]).toBe('e5')
    expect(d.recentErrors[19]).toBe('e24')
  })

  test('recentErrors empty when none recorded', () => {
    const d = collectDiagnostics('/')
    expect(d.recentErrors).toEqual([])
  })

  test('viewport guarded in non-DOM environment', () => {
    delete (globalThis as { window?: unknown }).window
    const d = collectDiagnostics('/')
    expect(d.viewport).toMatch(/^\d+x\d+$/)
  })
})
