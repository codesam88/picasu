import { describe, expect, test } from 'vitest'
import { databaseTimestampSchema } from '@type/schemas'

describe('databaseTimestampSchema', () => {
  test('parses assetId when present', () => {
    const input = {
      abstractData: {
        type: 'image' as const,
        id: 'abc123',
        pending: false,
        width: 100,
        height: 100,
        ext: 'jpg',
        size: 1024,
        tags: [],
        exifVec: {},
        rating: null,
        updateAt: 0,
        path: null
      },
      timestamp: 1700000000000,
      token: 'tok_123',
      assetId: 'asset_abc'
    }

    const result = databaseTimestampSchema.parse(input)
    expect(result.assetId).toBe('asset_abc')
    expect(result.token).toBe('tok_123')
    expect(result.timestamp).toBe(1700000000000)
    expect(result.abstractData.id).toBe('abc123')
  })

  test('rejects when assetId is missing', () => {
    const input = {
      abstractData: {
        type: 'image' as const,
        id: 'abc123',
        pending: false,
        width: 100,
        height: 100,
        ext: 'jpg',
        size: 1024,
        tags: [],
        exifVec: {},
        rating: null,
        updateAt: 0,
        path: null
      },
      timestamp: 1700000000000,
      token: 'tok_123'
    }

    expect(() => databaseTimestampSchema.parse(input)).toThrow()
  })

  test('two same-hash items with different assetId are distinguishable', () => {
    const base = {
      type: 'image' as const,
      id: 'same_hash_abc',
      pending: false,
      width: 100,
      height: 100,
      ext: 'jpg',
      size: 1024,
      tags: [],
      exifVec: {},
      rating: null,
      updateAt: 0,
      path: null
    }

    const item1 = {
      abstractData: {
        ...base,
        path: { file: '/path/a.jpg', modified: 1000, scanTime: 1000, isTrashed: false }
      },
      timestamp: 1700000000000,
      token: 'tok_a',
      assetId: 'asset_a'
    }

    const item2 = {
      abstractData: {
        ...base,
        path: { file: '/path/b.jpg', modified: 2000, scanTime: 2000, isTrashed: false }
      },
      timestamp: 1700000000001,
      token: 'tok_b',
      assetId: 'asset_b'
    }

    const result1 = databaseTimestampSchema.parse(item1)
    const result2 = databaseTimestampSchema.parse(item2)

    // Same content hash but different asset IDs
    expect(result1.abstractData.id).toBe(result2.abstractData.id)
    expect(result1.assetId).toBe('asset_a')
    expect(result2.assetId).toBe('asset_b')
    expect(result1.assetId).not.toBe(result2.assetId)
  })

  test('video type preserves assetId', () => {
    const input = {
      abstractData: {
        type: 'video' as const,
        id: 'vid_hash',
        pending: false,
        width: 1920,
        height: 1080,
        ext: 'mp4',
        size: 1024000,
        duration: 120,
        tags: [],
        exifVec: {},
        rating: null,
        updateAt: 0,
        path: null
      },
      timestamp: 1700000000000,
      token: 'tok_vid',
      assetId: 'asset_vid'
    }

    const result = databaseTimestampSchema.parse(input)
    expect(result.assetId).toBe('asset_vid')
    expect(result.abstractData.type).toBe('video')
  })

  test('album type preserves assetId', () => {
    const input = {
      abstractData: {
        type: 'album' as const,
        id: 'album_hash',
        pending: false,
        title: 'Test Album',
        startTime: 1700000000000,
        endTime: 1700000001000,
        lastModifiedTime: 1700000001000,
        cover: 'cover_hash',
        itemCount: 5,
        itemSize: 1024000,
        tags: [],
        shareList: {}
      },
      timestamp: 1700000000000,
      token: 'tok_album',
      assetId: 'asset_album'
    }

    const result = databaseTimestampSchema.parse(input)
    expect(result.assetId).toBe('asset_album')
    expect(result.abstractData.type).toBe('album')
  })

  test('album top-level coverHash is preserved when present', () => {
    const input = {
      abstractData: {
        type: 'album' as const,
        id: 'album_hash',
        pending: false,
        title: 'Test Album',
        startTime: 1700000000000,
        endTime: 1700000001000,
        lastModifiedTime: 1700000001000,
        cover: 'cover_asset_id',
        itemCount: 5,
        itemSize: 1024000,
        tags: [],
        shareList: {}
      },
      timestamp: 1700000000000,
      token: 'tok_album',
      assetId: 'asset_album',
      coverHash: 'cover_content_hash'
    }

    const result = databaseTimestampSchema.parse(input)
    expect(result.coverHash).toBe('cover_content_hash')
    expect(result.abstractData.type).toBe('album')
    if (result.abstractData.type === 'album') {
      expect(result.abstractData.cover).toBe('cover_asset_id')
    }
  })

  test('album top-level coverHash defaults to undefined when absent', () => {
    const input = {
      abstractData: {
        type: 'album' as const,
        id: 'album_hash',
        pending: false,
        title: 'Test Album',
        startTime: 1700000000000,
        endTime: 1700000001000,
        lastModifiedTime: 1700000001000,
        cover: 'cover_asset_id',
        itemCount: 5,
        itemSize: 1024000,
        tags: [],
        shareList: {}
      },
      timestamp: 1700000000000,
      token: 'tok_album',
      assetId: 'asset_album'
    }

    const result = databaseTimestampSchema.parse(input)
    expect(result.coverHash).toBeUndefined()
  })

  test('media types pass through top-level coverHash', () => {
    const input = {
      abstractData: {
        type: 'image' as const,
        id: 'img_hash',
        pending: false,
        width: 1920,
        height: 1080,
        ext: 'jpg',
        size: 1024000,
        path: null
      },
      timestamp: 1700000000000,
      token: 'tok_img',
      assetId: 'asset_img',
      coverHash: 'should_be_ignored'
    }

    const result = databaseTimestampSchema.parse(input)
    expect(result.abstractData.type).toBe('image')
    expect(result.coverHash).toBe('should_be_ignored')
  })

  test('favorite and archived flags are gone from the parsed asset', () => {
    const input = {
      abstractData: {
        type: 'image' as const,
        id: 'abc123',
        pending: false,
        width: 100,
        height: 100,
        ext: 'jpg',
        size: 1024,
        tags: [],
        exifVec: {},
        rating: null,
        updateAt: 0,
        path: null
      },
      timestamp: 1700000000000,
      token: 'tok_123',
      assetId: 'asset_abc'
    }

    const result = databaseTimestampSchema.parse(input)
    expect('isFavorite' in result.abstractData).toBe(false)
    expect('isArchived' in result.abstractData).toBe(false)
    expect(result.abstractData.id).toBe('abc123')
  })

  // The detail route serves the read-only "further data" bucket, keyed
  // `Group:Tag`; a list row never carries it. Both shapes have to survive the
  // parser, because the sidebar reads the field off the same parsed object the
  // list row was merged into.
  test('further metadata survives parsing, and defaults to empty when absent', () => {
    const base = {
      type: 'image' as const,
      id: 'abc123',
      pending: false,
      width: 100,
      height: 100,
      ext: 'jpg',
      size: 1024,
      tags: [],
      exifVec: {},
      rating: null,
      updateAt: 0,
      path: null
    }
    const withBucket = {
      abstractData: {
        ...base,
        furtherMetadata: {
          'IPTC:By-line': 'picasu fixture author',
          'XMP-xmp:CreatorTool': 'snapfab 1.0'
        }
      },
      timestamp: 1700000000000,
      token: 'tok_123',
      assetId: 'asset_abc'
    }
    const withoutBucket = {
      abstractData: base,
      timestamp: 1700000000000,
      token: 'tok_123',
      assetId: 'asset_abc'
    }

    const detail = databaseTimestampSchema.parse(withBucket)
    const lean = databaseTimestampSchema.parse(withoutBucket)
    expect(detail.abstractData.type).toBe('image')
    expect(lean.abstractData.type).toBe('image')
    if (detail.abstractData.type !== 'image' || lean.abstractData.type !== 'image') {
      throw new Error('both payloads are images')
    }

    expect(detail.abstractData.furtherMetadata).toEqual({
      'IPTC:By-line': 'picasu fixture author',
      'XMP-xmp:CreatorTool': 'snapfab 1.0'
    })
    expect(lean.abstractData.furtherMetadata).toEqual({})
  })
})
