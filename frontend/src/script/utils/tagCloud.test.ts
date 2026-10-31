import { describe, expect, test } from 'vitest'
import { tagCloudWeight, tagCloudFontSize } from './tagCloud'

// ── tagCloudWeight ────────────────────────────────────────────────────────────

describe('tagCloudWeight', () => {
  test('equal counts map to equal weights', () => {
    const counts = [2, 2, 5]
    const min = 2
    const max = 5
    const weights = counts.map((c) => tagCloudWeight(c, min, max))
    expect(weights[0]).toBe(weights[1])
    expect(weights[0]).not.toBe(weights[2])
  })

  test('larger count maps to larger weight (ordering preserved)', () => {
    const min = 1
    const max = 100
    const w1 = tagCloudWeight(1, min, max)
    const w10 = tagCloudWeight(10, min, max)
    const w100 = tagCloudWeight(100, min, max)
    expect(w1).toBeLessThan(w10)
    expect(w10).toBeLessThan(w100)
  })

  test('degenerate range (minCount === maxCount) returns 0.5', () => {
    expect(tagCloudWeight(7, 7, 7)).toBe(0.5)
    expect(tagCloudWeight(3, 5, 5)).toBe(0.5)
    expect(tagCloudWeight(5, 5, 3)).toBe(0.5)
  })

  test('weight output is clamped to [0, 1]', () => {
    // below min -> 0
    expect(tagCloudWeight(0, 5, 10)).toBe(0)
    // above max -> 1
    expect(tagCloudWeight(100, 1, 10)).toBe(1)
    // in-range stays within bounds
    const w = tagCloudWeight(3, 1, 10)
    expect(w).toBeGreaterThanOrEqual(0)
    expect(w).toBeLessThanOrEqual(1)
  })

  test('handles count <= 0 without throwing', () => {
    expect(() => tagCloudWeight(0, 0, 5)).not.toThrow()
    expect(() => tagCloudWeight(-1, 1, 10)).not.toThrow()
  })
})

// ── tagCloudFontSize ──────────────────────────────────────────────────────────

describe('tagCloudFontSize', () => {
  test('weight 0 -> minRem', () => {
    expect(tagCloudFontSize(0)).toBe(0.8)
  })

  test('weight 1 -> maxRem', () => {
    expect(tagCloudFontSize(1)).toBe(1.6)
  })

  test('weight 0.5 -> midpoint', () => {
    expect(tagCloudFontSize(0.5)).toBe(1.2)
  })

  test('defaults are 0.8 / 1.6', () => {
    expect(tagCloudFontSize(0, 0.8, 1.6)).toBe(0.8)
    expect(tagCloudFontSize(1, 0.8, 1.6)).toBe(1.6)
  })

  test('custom range interpolates linearly', () => {
    expect(tagCloudFontSize(0.5, 1.0, 2.0)).toBe(1.5)
  })

  test('out-of-range weight clamps', () => {
    expect(tagCloudFontSize(-0.5)).toBe(0.8)
    expect(tagCloudFontSize(1.5)).toBe(1.6)
  })
})
