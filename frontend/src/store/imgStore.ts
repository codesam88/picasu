import { IsolationId } from '@type/types'
import { defineStore } from 'pinia'

/**
 * One live thumbnail URL per (asset, display mode) slot. `cacheKey` records the
 * requested output box (see `thumbnailCacheKey` in the image worker); it is the
 * reuse test: same key means the existing URL is still valid, a different key
 * means the tile must be re-encoded at the new size.
 */
export interface ThumbnailUrlEntry {
  url: string
  cacheKey: string
}

/**
 * Thumbnails are keyed by asset identity, not by grid index: a refresh can
 * reorder or re-slice rows, moving the same asset to a different index while
 * its bytes stay identical. The display mode is part of the slot because a
 * cover-fit album thumbnail and the same asset's contain-fit tile request
 * different output and must not evict each other.
 */
export function thumbnailSlot(assetId: string, albumMode: boolean): string {
  return `${assetId}|${albumMode ? 'cover' : 'contain'}`
}

function revokeUrl(url: string): void {
  URL.revokeObjectURL(url)
}

export const useImgStore = (isolationId: IsolationId) =>
  defineStore('imgStore' + isolationId, {
    state: (): {
      imgUrl: Map<string, ThumbnailUrlEntry> // assetId|mode -> { url, cacheKey }
      imgOriginal: Map<number, string> // dataIndex -> blobUrl
    } => ({
      imgUrl: new Map(),
      imgOriginal: new Map()
    }),
    actions: {
      /**
       * Returns the live URL only when it was produced for `cacheKey`.
       * A missing or differently-keyed entry is a cache miss, so the caller
       * re-queues the tile and the worker re-encodes at the new box.
       */
      getImgUrl(assetId: string, albumMode: boolean, cacheKey: string): string | undefined {
        const entry = this.imgUrl.get(thumbnailSlot(assetId, albumMode))
        return entry?.cacheKey === cacheKey ? entry.url : undefined
      },

      /**
       * Stores a freshly received URL, revoking the previous one for the same
       * slot. A duplicate response for the key already held is dropped and its
       * URL revoked, so exactly one live object URL exists per cache key.
       */
      setImgUrl(assetId: string, albumMode: boolean, cacheKey: string, url: string): void {
        const slot = thumbnailSlot(assetId, albumMode)
        const previous = this.imgUrl.get(slot)

        if (previous !== undefined) {
          if (previous.cacheKey === cacheKey) {
            revokeUrl(url)
            return
          }
          revokeUrl(previous.url)
        }

        this.imgUrl.set(slot, { url, cacheKey })
      },

      setImgOriginal(index: number, url: string): void {
        const previous = this.imgOriginal.get(index)
        if (previous === url) return
        if (previous !== undefined) revokeUrl(previous)
        this.imgOriginal.set(index, url)
      },

      // Releasing the worker also abandons every URL it minted, so revoke both
      // maps instead of dropping the references (the old behaviour leaked the
      // underlying blobs until the page was unloaded).
      clearAll() {
        for (const entry of this.imgUrl.values()) revokeUrl(entry.url)
        for (const url of this.imgOriginal.values()) revokeUrl(url)
        this.imgUrl.clear()
        this.imgOriginal.clear()
      }
    }
  })()
