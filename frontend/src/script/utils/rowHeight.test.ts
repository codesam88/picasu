import { describe, expect, test } from 'vitest'
import { rowBaseHeight } from './rowHeight'

describe('rowBaseHeight', () => {
  test('the setting is the row height on wide windows', () => {
    expect(rowBaseHeight(1920, 250)).toBe(250)
    expect(rowBaseHeight(900, 450)).toBe(450)
    // guard does not bind at exactly twice the setting
    expect(rowBaseHeight(500, 250)).toBe(250)
  })

  test('rows shrink to half the window width on narrow screens', () => {
    expect(rowBaseHeight(400, 250)).toBe(200)
    expect(rowBaseHeight(0, 250)).toBe(0)
  })

  test('the narrow-screen result is rounded', () => {
    expect(rowBaseHeight(401, 300)).toBe(201)
  })
})
