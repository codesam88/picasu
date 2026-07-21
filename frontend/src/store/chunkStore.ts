import { IsolationId, DisplayElement } from '@type/types'
import { defineStore } from 'pinia'

export const useChunkStore = (isolationId: IsolationId) =>
  defineStore('chunkStore' + isolationId, {
    state: (): {
      chunks: Map<number, DisplayElement[]> // Map<chunkIndex, images>
      totalImages: number
      loadedChunks: Set<number>
      currentChunkIndex: number
    } => ({
      chunks: new Map(),
      totalImages: 0,
      loadedChunks: new Set(),
      currentChunkIndex: 0
    }),
    actions: {
      setChunk(chunkIndex: number, images: DisplayElement[]) {
        this.chunks.set(chunkIndex, images)
        this.loadedChunks.add(chunkIndex)
      },
      setTotalImages(total: number) {
        this.totalImages = total
      },
      clearAll() {
        this.chunks.clear()
        this.totalImages = 0
        this.loadedChunks.clear()
        this.currentChunkIndex = 0
      }
    }
  })()
