import { describe, expect, test, beforeEach, afterEach } from 'vitest'
import { installErrorLog, getRecentErrors, clearErrorLog } from './errorLog'

interface FakeWindow {
  onerror: ((message: unknown) => boolean) | null
  addEventListener: (type: string, listener: (event: unknown) => void) => void
}

describe('errorLog', () => {
  beforeEach(() => {
    clearErrorLog()
  })

  afterEach(() => {
    delete (globalThis as { window?: FakeWindow }).window
    clearErrorLog()
  })

  test('cap respected: keeps last 20', () => {
    const w: FakeWindow = {
      onerror: null,
      addEventListener: () => {
        void 0
      }
    }
    ;(globalThis as { window?: FakeWindow }).window = w
    installErrorLog()
    for (let i = 0; i < 25; i += 1) {
      if (w.onerror !== null) w.onerror(`err ${i}`)
    }
    const recent = getRecentErrors()
    expect(recent).toHaveLength(20)
    expect(recent[0]).toBe('err 5')
    expect(recent[19]).toBe('err 24')
  })

  test('getRecentErrors returns a copy', () => {
    const w: FakeWindow = {
      onerror: null,
      addEventListener: () => {
        void 0
      }
    }
    ;(globalThis as { window?: FakeWindow }).window = w
    installErrorLog()
    if (w.onerror !== null) w.onerror('boom')
    const a = getRecentErrors()
    expect(a).toEqual(['boom'])
    a.push('x')
    expect(getRecentErrors()).toEqual(['boom'])
  })

  test('installErrorLog is no-op and never throws without window', () => {
    delete (globalThis as { window?: FakeWindow }).window
    expect(() => {
      installErrorLog()
    }).not.toThrow()
    expect(getRecentErrors()).toEqual([])
  })

  test('listeners do not throw on weird input', () => {
    const w: FakeWindow = {
      onerror: null,
      addEventListener: () => {
        void 0
      }
    }
    ;(globalThis as { window?: FakeWindow }).window = w
    installErrorLog()
    expect(() => {
      if (w.onerror !== null) {
        w.onerror(undefined)
        w.onerror(null)
        w.onerror({ message: 'objmsg' })
      }
    }).not.toThrow()
  })

  test('unhandledrejection appends message only', () => {
    const listeners: Record<string, ((e: unknown) => void)[]> = {}
    const w: FakeWindow = {
      onerror: null,
      addEventListener: (type: string, cb: (e: unknown) => void) => {
        const list = listeners[type]
        if (list === undefined) {
          listeners[type] = [cb]
        } else {
          list.push(cb)
        }
      }
    }
    ;(globalThis as { window?: FakeWindow }).window = w
    installErrorLog()
    for (const cb of listeners.unhandledrejection ?? []) cb({ reason: new Error('rejected') })
    expect(getRecentErrors()).toContain('rejected')
  })
})
