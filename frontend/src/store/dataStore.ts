import type { EnrichedUnifiedData, IsolationId } from '@type/types'
import { defineStore } from 'pinia'

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
