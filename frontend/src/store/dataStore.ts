import type { EnrichedUnifiedData, IsolationId, UnifiedData } from '@type/types'
import { defineStore } from 'pinia'
import { thumbHashToDataURL } from 'thumbhash'

export const useDataStore = (isolationId: IsolationId) =>
  defineStore('DataStore' + isolationId, {
    state: (): {
      data: Map<number, EnrichedUnifiedData> // dataIndex -> data
      assetIdMapData: Map<string, number> // assetId -> dataIndex (authoritative identity map)
      batchFetched: Map<number, boolean> // Tracks the batches of image metadata that have been fetched
    } => ({
      data: new Map(),
      assetIdMapData: new Map(),
      batchFetched: new Map()
    }),
    actions: {
      // Should be cleared when the layout is changed
      clearAll() {
        this.data.clear()
        this.assetIdMapData.clear()
        this.batchFetched.clear()
      },
      /**
       * Merge a detail-endpoint payload (GET /get/metadata/{assetId}) into the
       * list row at `index`.
       *
       * List rows are lean: tags, EXIF, description, rating, and the
       * favorite/archived flags are absent until fetched. Only those
       * metadata fields are overwritten — identity fields (id, dimensions,
       * alias, album, assetId, timestamp, thumbhashUrl) stay as the list
       * provided them. Returns false when the row is missing or the detail
       * payload type does not match the row type.
       */
      mergeMetadata(index: number, detail: UnifiedData): boolean {
        const data = this.data.get(index)
        if (data === undefined) {
          return false
        }
        if (data.type !== detail.type) {
          return false
        }
        if (detail.type === 'album' || data.type === 'album') {
          // Album rows are already served with full metadata; nothing to merge.
          return false
        }
        data.tags = detail.tags
        data.exif = detail.exif
        data.description = detail.description
        data.rating = detail.rating
        data.isFavorite = detail.isFavorite
        data.isArchived = detail.isArchived
        data.updateAt = detail.updateAt
        data.pending = detail.pending
        if (data.type === 'image' && detail.type === 'image') {
          data.phash = detail.phash
        }
        if (detail.thumbhash !== null) {
          data.thumbhash = detail.thumbhash
          data.thumbhashUrl = thumbHashToDataURL(detail.thumbhash)
        }
        return true
      },
      addTags(index: number, tags: string[]): boolean {
        const data = this.data.get(index)
        if (!data) {
          // Index does not exist
          return false
        }

        tags.forEach((tag) => {
          if (!data.tags.includes(tag)) {
            data.tags.push(tag)
          }
        })
        return true
      },
      removeTags(index: number, tags: string[]): boolean {
        const data = this.data.get(index)
        if (!data) {
          return false
        }

        data.tags = data.tags.filter((tag) => !tags.includes(tag))
        return true
      },
      setAlbum(index: number, album: string | null): boolean {
        const data = this.data.get(index)
        if (!data) {
          return false
        }

        if (data.type === 'image' || data.type === 'video') {
          data.album = album
          return true
        }

        return false
      }
    }
  })()
