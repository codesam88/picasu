import { describe, expect, test } from 'vitest'
import { getSrc } from './getter'

describe('getSrc', () => {
  test('uses content hash for compressed files', () => {
    const result = getSrc('abc123', false, 'jpg', 1700000000000)
    expect(result).toBe('/object/compressed/ab/abc123.jpg?updated_at=1700000000000')
  })

  test('uses content hash for original files when assetId is not provided', () => {
    const result = getSrc('abc123', true, 'jpg', 1700000000000)
    expect(result).toBe('/object/imported/ab/abc123.jpg?updated_at=1700000000000')
  })

  test('uses assetId for original files when assetId is provided', () => {
    const result = getSrc('abc123', true, 'jpg', 1700000000000, 'asset_xyz')
    expect(result).toBe('/object/imported/as/asset_xyz.jpg?updated_at=1700000000000')
  })

  test('falls back to content hash for compressed files even when assetId is provided', () => {
    const result = getSrc('abc123', false, 'jpg', 1700000000000, 'asset_xyz')
    expect(result).toBe('/object/compressed/ab/abc123.jpg?updated_at=1700000000000')
  })
})
