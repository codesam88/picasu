import { RouteLocationNormalizedLoaded, Router } from 'vue-router'
import { inject } from 'vue'
import { useDataStore } from '@/store/dataStore'
import { escapeAndWrap } from '@utils/escape'
import { useShareStore } from '@/store/shareStore'
import { IsolationId } from '@type/types'

export function getIsolationIdByRoute(_route: RouteLocationNormalizedLoaded): IsolationId {
  return 'mainId'
}

export function getAssetIndexDataFromRoute(route: RouteLocationNormalizedLoaded) {
  const isolationId = getIsolationIdByRoute(route)
  const storeData = useDataStore(isolationId)

  let assetId: string

  if (typeof route.params.assetId === 'string') {
    assetId = route.params.assetId
  } else {
    return undefined
  }

  const index = storeData.assetIdMapData.get(assetId)

  if (index === undefined) {
    return undefined
  }

  const data = storeData.data.get(index)

  if (data === undefined) {
    return undefined
  }

  return { assetId: assetId, index: index, data: data }
}

export function getArrayValue<T>(array: T[], index: number): T {
  const result = array[index]
  if (result === undefined) {
    throw new RangeError(`Index ${index} is out of bounds for array of length ${array.length}`)
  } else {
    return result
  }
}

/**
 * Retrieves an injected value and ensures it's not undefined.
 * @param key - The injection key.
 * @returns The injected value of type T.
 * @throws {RangeError} If the injected value is undefined.
 */
export function getInjectValue<T>(key: string | symbol): T {
  const result = inject<T>(key)
  if (result === undefined) {
    throw new RangeError(`Injection for key "${String(key)}" is undefined.`)
  }
  return result
}

/**
 * Retrieves a value from a Map and ensures it's not undefined.
 * @param map - The Map to retrieve the value from.
 * @param key - The key whose associated value is to be returned.
 * @returns The value associated with the specified key.
 * @throws {RangeError} If the key does not exist in the Map.
 */
export function getMapValue<K, V>(map: Map<K, V>, key: K): V {
  const value = map.get(key)
  if (value === undefined) {
    throw new RangeError(`No value found for key "${String(key)}" in the map.`)
  }
  return value
}

export function getScrollUpperBound(totalHeight: number, windowHeight: number): number {
  return totalHeight - windowHeight - 4
}

export async function searchByTag(tag: string, router: Router) {
  const { meta, params } = router.currentRoute.value
  const searchQuery = { search: `tag:${escapeAndWrap(tag)}` }

  // if the current baseName is 'share', navigate back to the share root page
  if (meta.baseName === 'share') {
    const albumId = params.albumId as string
    const shareId = params.shareId as string
    await router.push({
      name: 'share',
      params: { albumId, shareId },
      query: searchQuery
    })
  } else {
    await router.push({
      name: 'timeline',
      query: searchQuery
    })
  }
}

/**
 * Extracts the serving ID from a full URL (last path segment before the
 * extension): the content hash for compressed URLs, the asset ID for
 * original-file URLs.
 */
export function extractServingIdFromAbsoluteUrl(url: URL): string | null {
  const segments = url.pathname.split('/').filter(Boolean)
  const lastSegment = segments.pop()

  return lastSegment?.split('.').shift() ?? null
}

/**
 * Extracts the serving ID from a relative path (last path segment before the
 * extension): the content hash for compressed URLs, the asset ID for
 * original-file URLs.
 */
export function extractServingIdFromPath(path: string): string | null {
  const segments = path.split('/').filter(Boolean)
  const lastSegment = segments.pop()

  return lastSegment?.split('.').shift() ?? null
}

/**
 * Serving IDs for an album-cover fetch.
 *
 * `hash` is the cover's content hash — the segment of the content-addressed
 * compressed URL and the value `GuardHash` checks against the serving token's
 * `hash` claim. `assetId` is the cover asset's identity — the blob-cache key
 * and the token-store key. Returns `null` when either piece is missing; never
 * falls back to the asset id for the hash slot.
 */
export function coverServingIds(
  cover: string | null | undefined,
  coverHash: string | null | undefined
): { hash: string; assetId: string } | null {
  if (cover == null || coverHash == null) return null
  return { hash: coverHash, assetId: cover }
}

export function getSrc(
  hash: string,
  original: boolean,
  ext: string,
  updatedAt: number,
  assetId?: string
) {
  const compressedOrImported = original ? 'imported' : 'compressed'
  // Original files are asset-addressed (backend resolves by assetId);
  // compressed files are content-addressed (shared across duplicates) —
  // GuardHash validates this segment against the token's hash claim.
  if (original) {
    if (assetId === undefined) {
      throw new Error('assetId is required for original file URLs')
    }
    const basePath = `/object/${compressedOrImported}/${assetId.slice(0, 2)}/${assetId}.${ext}`
    return `${basePath}?updated_at=${updatedAt}`
  }
  const basePath = `/object/${compressedOrImported}/${hash.slice(0, 2)}/${hash}.${ext}`
  return `${basePath}?updated_at=${updatedAt}`
}

export function getSrcOriginal(
  hash: string,
  original: boolean,
  ext: string,
  updatedAt: number,
  assetId?: string
) {
  const shareStore = useShareStore('mainId')
  const baseSrc = getSrc(hash, original, ext, updatedAt, assetId)

  if (typeof shareStore.albumId === 'string' && typeof shareStore.shareId === 'string') {
    const separator = baseSrc.includes('?') ? '&' : '?'
    return `${baseSrc}${separator}albumId=${shareStore.albumId}&shareId=${shareStore.shareId}`
  } else {
    return baseSrc
  }
}
