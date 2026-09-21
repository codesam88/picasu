import { describe, expect, test } from 'vitest'
import { getSrc } from './getter'

describe('getSrc', () => {
  test('uses content hash for compressed files', () => {
    const result = getSrc('abc123', false, 'jpg', 1700000000000)
    expect(result).toBe('/object/compressed/ab/abc123.jpg?updated_at=1700000000000')
  })

  test('uses assetId for original files when assetId is provided', () => {
    const result = getSrc('abc123', true, 'jpg', 1700000000000, 'asset_xyz')
    expect(result).toBe('/object/imported/as/asset_xyz.jpg?updated_at=1700000000000')
  })

  test('throws when assetId is missing for original files', () => {
    expect(() => getSrc('abc123', true, 'jpg', 1700000000000)).toThrow(
      'assetId is required for original file URLs'
    )
  })

  test('uses content hash for compressed files even when assetId is provided', () => {
    const result = getSrc('abc123', false, 'jpg', 1700000000000, 'asset_xyz')
    expect(result).toBe('/object/compressed/ab/abc123.jpg?updated_at=1700000000000')
  })
})
