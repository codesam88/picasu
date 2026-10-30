import { describe, expect, test } from 'vitest'
import { DEFAULT_EXPIRY_HOURS, parseExpiryInput } from './expiryInput'

const MAX = 8760

describe('parseExpiryInput', () => {
  test('plain units translate to hours', () => {
    expect(parseExpiryInput('24h', MAX)).toEqual({ ok: true, hours: 24 })
    expect(parseExpiryInput('7d', MAX)).toEqual({ ok: true, hours: 168 })
    expect(parseExpiryInput('12m', MAX)).toEqual({ ok: true, hours: 8640 })
  })

  test('a 30-day month keeps the one-year bound reachable', () => {
    expect(parseExpiryInput('12m', MAX)).toEqual({ ok: true, hours: 8640 })
    expect(parseExpiryInput('13m', MAX).ok).toBe(false)
  })

  test('whitespace is stripped before parsing', () => {
    expect(parseExpiryInput('  7d  ', MAX)).toEqual({ ok: true, hours: 168 })
    expect(parseExpiryInput('7 d', MAX)).toEqual({ ok: true, hours: 168 })
  })

  test('units are case-insensitive', () => {
    expect(parseExpiryInput('7D', MAX)).toEqual({ ok: true, hours: 168 })
  })

  test('empty input falls back to the conventional 7 days', () => {
    expect(parseExpiryInput('', MAX)).toEqual({ ok: true, hours: DEFAULT_EXPIRY_HOURS })
    expect(parseExpiryInput('   ', MAX)).toEqual({ ok: true, hours: DEFAULT_EXPIRY_HOURS })
  })

  test('zero means never expires', () => {
    expect(parseExpiryInput('0h', MAX)).toEqual({ ok: true, hours: 0 })
    expect(parseExpiryInput('0d', MAX)).toEqual({ ok: true, hours: 0 })
  })

  test('the bound itself is accepted, one over is not', () => {
    expect(parseExpiryInput('8760h', MAX)).toEqual({ ok: true, hours: 8760 })
    expect(parseExpiryInput('8761h', MAX).ok).toBe(false)
    expect(parseExpiryInput('366d', MAX).ok).toBe(false)
  })

  test('anything but <integer><unit> is rejected', () => {
    for (const raw of ['abc', '7', '7x', 'h', '-5d', '+5d', '5.5d', 'd7', '7dd', '1y', '1w']) {
      expect(parseExpiryInput(raw, MAX).ok, raw).toBe(false)
    }
  })

  test('absurd magnitudes are rejected, never computed', () => {
    expect(parseExpiryInput(`${'9'.repeat(40)}h`, MAX).ok).toBe(false)
  })
})
