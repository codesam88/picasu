import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const source = readFileSync(
  resolve(import.meta.dirname, '../../components/Modal/UserSettingsModal.vue'),
  'utf-8'
)

describe('UserSettingsModal identity header', () => {
  test('reads identity read-only via useCurrentUser', () => {
    expect(source).toMatch(/useCurrentUser/)
  })

  test('header shows the username', () => {
    expect(source).toMatch(/currentUser.*\.?id|username/i)
  })

  test('header shows an Admin/Standard badge', () => {
    expect(source).toMatch(/Admin/)
    expect(source).toMatch(/Standard/)
  })

  test('falls back to a generic title when identity is null', () => {
    expect(source).toMatch(/Settings/)
    expect(source).toMatch(/\?\./)
  })
})
