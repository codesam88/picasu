/**
 * Cache primitives shared by the image worker: the LRU used for fetched
 * source blobs and re-encoded thumbnails, the thumbnail key shape, and the
 * hit-or-encode helper.
 *
 * Kept free of worker globals so the cache-hit path can be unit-tested under
 * node vitest without importing the worker realm (`self`).
 */

/**
 * Upper bounds are entry counts, not bytes: typical grid tiles are a few
 * tens of KB after re-encode, so 500 thumbnails stay well under ~30 MB.
 * Source blobs are the server's compressed variants and are larger, hence
 * the smaller cap. Both are LRU so a long scroll can revisit recent tiles
 * without paying the network/encode cost again.
 */
export const RESIZED_BLOB_CACHE_LIMIT = 500
export const SOURCE_BLOB_CACHE_LIMIT = 200

export interface ThumbnailCacheKeyParts {
  assetId: string
  width: number
  height: number
  devicePixelRatio: number
  albumMode: boolean
}

/**
 * The encoded output depends only on the asset and the requested box, so the
 * same (assetId, width, height, dpr, albumMode) request always maps to the
 * same bytes, independent of the tile index it is displayed at. `albumMode`
 * is included because cover-fit uses a different max-box calculation than
 * contain-fit.
 */
export function thumbnailCacheKey(parts: ThumbnailCacheKeyParts): string {
  const mode = parts.albumMode ? 'cover' : 'contain'
  return `${parts.assetId}|${parts.width}x${parts.height}@${parts.devicePixelRatio}|${mode}`
}

export class LruCache<K, V> {
  readonly #entries = new Map<K, V>()
  readonly #limit: number

  constructor(limit: number) {
    this.#limit = Math.max(1, Math.floor(limit))
  }

  get size(): number {
    return this.#entries.size
  }

  get(key: K): V | undefined {
    const value = this.#entries.get(key)
    if (value === undefined) return undefined
    // Refresh recency on read.
    this.#entries.delete(key)
    this.#entries.set(key, value)
    return value
  }

  set(key: K, value: V): void {
    this.#entries.delete(key)
    this.#entries.set(key, value)
    while (this.#entries.size > this.#limit) {
      const oldest = this.#entries.keys().next().value
      if (oldest === undefined) break
      this.#entries.delete(oldest)
    }
  }

  delete(key: K): void {
    this.#entries.delete(key)
  }

  deleteWhere(predicate: (key: K) => boolean): void {
    for (const key of this.#entries.keys()) {
      if (predicate(key)) this.#entries.delete(key)
    }
  }
}

/**
 * Returns the cached thumbnail for `key`, calling `encode` only on a miss.
 * The worker handler routes every request through this so a cache hit skips
 * `createImageBitmap` and the resize/re-encode work entirely.
 */
export async function getOrEncodeThumbnail(
  cache: LruCache<string, Blob>,
  key: string,
  encode: () => Promise<Blob>
): Promise<Blob> {
  const cached = cache.get(key)
  if (cached !== undefined) return cached
  const encoded = await encode()
  cache.set(key, encoded)
  return encoded
}
