import { describe, expect, test } from 'vitest'
import { coverServingIds, getSrc } from './getter'

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

  test('album cover thumbnail URL uses content hash, not asset_id', () => {
    const coverAssetId = 'album_cover_asset_id_abc'
    const coverContentHash = 'real_content_hash_xyz'
    const result = getSrc(coverContentHash, false, 'jpg', 1700000000000)
    expect(result).toBe('/object/compressed/re/real_content_hash_xyz.jpg?updated_at=1700000000000')
    expect(result).not.toContain(coverAssetId)
  })
})

describe('coverServingIds', () => {
  test('compressed URL id is the cover content hash, identity is the cover asset id', () => {
    const ids = coverServingIds('cover_asset_1', 'cover_blake3_hash')
    expect(ids).toEqual({ hash: 'cover_blake3_hash', assetId: 'cover_asset_1' })
  })

  test('returns null when the cover asset id is missing', () => {
    expect(coverServingIds(null, 'cover_blake3_hash')).toBeNull()
    expect(coverServingIds(undefined, 'cover_blake3_hash')).toBeNull()
  })

  test('returns null when the cover content hash is missing', () => {
    // Never fall back to the asset id: /object/compressed is
    // content-addressed and GuardHash validates the hash claim.
    expect(coverServingIds('cover_asset_1', null)).toBeNull()
    expect(coverServingIds('cover_asset_1', undefined)).toBeNull()
  })
})
