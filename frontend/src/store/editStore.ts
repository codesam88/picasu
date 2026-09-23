import { IsolationId } from '@type/types'
import { defineStore } from 'pinia'

export const useEditStore = (isolationId: IsolationId) =>
  defineStore('editStore' + isolationId, {
    state: (): {
      processingRegenerate: Set<string>
      rotationCounts: Map<string, number>
      rotationQueue: Map<string, Promise<void>>
    } => ({
      processingRegenerate: new Set(),
      rotationCounts: new Map(),
      rotationQueue: new Map()
    }),
    actions: {
      async queueRotate(assetId: string, task: () => Promise<void>) {
        // Get the current promise chain for this asset, or start a new one
        // eslint-disable-next-line @typescript-eslint/prefer-nullish-coalescing
        const previousTask = this.rotationQueue.get(assetId) || Promise.resolve()

        // Chain the new task to run after the previous one completes
        const newTask = previousTask
          .then(() => task())
          .catch((error: unknown) => {
            console.error(`Rotation task failed for assetId ${assetId}:`, error)
          })

        // Update the queue with the new tail of the chain
        this.rotationQueue.set(assetId, newTask)

        // Wait for this specific task to finish (optional, depending on if caller needs to await)
        await newTask
      },
      addRegenerate(assetId: string) {
        this.processingRegenerate.add(assetId)
      },
      removeRegenerate(assetId: string) {
        this.processingRegenerate.delete(assetId)
      },
      hasRegenerate(assetId: string) {
        return this.processingRegenerate.has(assetId)
      },
      incrementRotation(assetId: string) {
        // eslint-disable-next-line @typescript-eslint/strict-boolean-expressions, @typescript-eslint/prefer-nullish-coalescing
        const count = this.rotationCounts.get(assetId) || 0
        this.rotationCounts.set(assetId, count + 1)
      }
    }
  })()
