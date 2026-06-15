import { EnrichedUnifiedData, IsolationId } from '@type/types'
import { defineStore } from 'pinia'
import { useDataStore } from './dataStore'
import { useTagStore } from './tagStore'

export interface EditTagsPayload {
  indexSet: Set<number>
  addTagsArray: string[]
  removeTagsArray: string[]
  timestamp: number
}

export const useOptimisticStore = (isolationId: IsolationId) =>
  defineStore('optimisticUpdateStore' + isolationId, {
    state: (): {
      backupData: Map<number, EnrichedUnifiedData> // dataIndex -> data
      queueTagsUpdate: EditTagsPayload[]
    } => ({
      backupData: new Map(),
      queueTagsUpdate: []
    }),
    actions: {
      clearAll() {
        this.backupData.clear()
        this.queueTagsUpdate = []
      },
      optimisticUpdateTags(payload: EditTagsPayload, pushIntoQueue: boolean) {
        const dataStore = useDataStore(isolationId)
        for (const index of dataStore.data.keys()) {
          if (payload.indexSet.has(index)) {
            const addTagsResult = dataStore.addTags(index, payload.addTagsArray)

            const removeTagsResult = dataStore.removeTags(index, payload.removeTagsArray)
            if (addTagsResult && removeTagsResult) {
              payload.indexSet.delete(index)
            }
          }
        }

        // Optimistically add newly created tags to the tagStore so they appear
        // immediately in combobox dropdowns without waiting for a server round-trip.
        const tagStore = useTagStore(isolationId)
        for (const tag of payload.addTagsArray) {
          if (!tagStore.tags.some((t) => t.tag === tag)) {
            tagStore.tags.push({ tag, number: 1 })
          }
        }
        if (payload.addTagsArray.length > 0) {
          tagStore.tags.sort((a, b) => a.tag.localeCompare(b.tag))
        }

        if (
          pushIntoQueue && // only the new task should be pushed
          payload.indexSet.size !== 0
        ) {
          // some data has not been fetched yet
          this.queueTagsUpdate.push(payload)
        }
      },
      selfUpdate() {
        this.queueTagsUpdate.forEach((payload) => {
          this.optimisticUpdateTags(payload, false)
        })
      }
    }
  })()
