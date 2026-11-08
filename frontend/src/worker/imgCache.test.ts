import { describe, expect, test, vi } from 'vitest'
import {
  getOrEncodeThumbnail,
  LruCache,
  RESIZED_BLOB_CACHE_LIMIT,
  SOURCE_BLOB_CACHE_LIMIT,
  thumbnailCacheKey
} from './imgCache'

const baseKey = {
  assetId: 'asset-1',
  width: 320,
  height: 240,
  devicePixelRatio: 2,
  albumMode: false
}

describe('LruCache', () => {
  test('evicts the least-recently-used entry once the bound is exceeded', () => {
    const cache = new LruCache<string, number>(2)
    cache.set('a', 1)
    cache.set('b', 2)
    // Read 'a' so 'b' becomes the oldest entry.
    expect(cache.get('a')).toBe(1)
    cache.set('c', 3)

    expect(cache.size).toBe(2)
    expect(cache.get('b')).toBeUndefined()
    expect(cache.get('a')).toBe(1)
    expect(cache.get('c')).toBe(3)
  })

  test('overwriting an existing key keeps the bound and the new value', () => {
    const cache = new LruCache<string, number>(1)
    cache.set('a', 1)
    cache.set('a', 2)

    expect(cache.size).toBe(1)
    expect(cache.get('a')).toBe(2)
  })

  test('deleteWhere removes only matching keys', () => {
    const cache = new LruCache<string, number>(4)
    cache.set('asset-1|a', 1)
    cache.set('asset-1|b', 2)
    cache.set('asset-2|a', 3)

    cache.deleteWhere((key) => key.startsWith('asset-1|'))

    expect(cache.size).toBe(1)
    expect(cache.get('asset-2|a')).toBe(3)
  })

  test('declared cache limits are positive', () => {
    expect(RESIZED_BLOB_CACHE_LIMIT).toBeGreaterThan(0)
    expect(SOURCE_BLOB_CACHE_LIMIT).toBeGreaterThan(0)
  })
})

describe('thumbnailCacheKey', () => {
  test('same asset and requested box produce the same key', () => {
    expect(thumbnailCacheKey(baseKey)).toBe(thumbnailCacheKey({ ...baseKey }))
  })

  test('dimensions, dpr, and album mode each change the key', () => {
    const key = thumbnailCacheKey(baseKey)
    expect(thumbnailCacheKey({ ...baseKey, width: 640 })).not.toBe(key)
    expect(thumbnailCacheKey({ ...baseKey, height: 480 })).not.toBe(key)
    expect(thumbnailCacheKey({ ...baseKey, devicePixelRatio: 1 })).not.toBe(key)
    expect(thumbnailCacheKey({ ...baseKey, albumMode: true })).not.toBe(key)
  })
})

describe('getOrEncodeThumbnail', () => {
  test('a cache hit skips the encode callback', async () => {
    const cache = new LruCache<string, Blob>(4)
    const key = thumbnailCacheKey(baseKey)
    const encode = vi.fn(() => Promise.resolve(new Blob(['thumbnail'])))

    const first = await getOrEncodeThumbnail(cache, key, encode)
    const second = await getOrEncodeThumbnail(cache, key, encode)

    expect(encode).toHaveBeenCalledTimes(1)
    expect(second).toBe(first)
    expect(cache.size).toBe(1)
  })

  test('a different dpr re-encodes', async () => {
    const cache = new LruCache<string, Blob>(4)
    const encode = vi.fn(() => Promise.resolve(new Blob(['thumbnail'])))

    await getOrEncodeThumbnail(cache, thumbnailCacheKey(baseKey), encode)
    await getOrEncodeThumbnail(
      cache,
      thumbnailCacheKey({ ...baseKey, devicePixelRatio: 1 }),
      encode
    )

    expect(encode).toHaveBeenCalledTimes(2)
    expect(cache.size).toBe(2)
  })

  test('a changed output box re-encodes while the old box stays cached', async () => {
    const cache = new LruCache<string, Blob>(4)
    const encode = vi.fn(() => Promise.resolve(new Blob(['thumbnail'])))

    const smallKey = thumbnailCacheKey(baseKey)
    const largeKey = thumbnailCacheKey({ ...baseKey, width: 640, height: 480 })

    const small = await getOrEncodeThumbnail(cache, smallKey, encode)
    const large = await getOrEncodeThumbnail(cache, largeKey, encode)

    expect(encode).toHaveBeenCalledTimes(2)
    expect(large).not.toBe(small)
    // The old box survives a settings change, so resizing back is free.
    expect(await getOrEncodeThumbnail(cache, smallKey, encode)).toBe(small)
    expect(encode).toHaveBeenCalledTimes(2)
  })

  test('an asset change at the same index re-encodes under the new asset key', async () => {
    const cache = new LruCache<string, Blob>(4)
    const encode = vi.fn(() => Promise.resolve(new Blob(['thumbnail'])))

    await getOrEncodeThumbnail(cache, thumbnailCacheKey(baseKey), encode)
    await getOrEncodeThumbnail(cache, thumbnailCacheKey({ ...baseKey, assetId: 'asset-2' }), encode)

    expect(encode).toHaveBeenCalledTimes(2)
    expect(cache.size).toBe(2)
  })

  test('purging an asset removes every cached size for it and keeps other assets', () => {
    const cache = new LruCache<string, Blob>(8)
    cache.set(thumbnailCacheKey(baseKey), new Blob(['small']))
    cache.set(thumbnailCacheKey({ ...baseKey, width: 640, height: 480 }), new Blob(['large']))
    cache.set(thumbnailCacheKey({ ...baseKey, assetId: 'asset-2' }), new Blob(['other']))

    cache.deleteWhere((key) => key.startsWith(`${baseKey.assetId}|`))

    expect(cache.size).toBe(1)
    expect(cache.get(thumbnailCacheKey({ ...baseKey, assetId: 'asset-2' }))).toBeDefined()
  })
})
