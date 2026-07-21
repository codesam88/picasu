import { useDataStore } from '@/store/dataStore'
import { IsolationId, SlicedData } from '@type/types'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useMessageStore } from '@/store/messageStore'
import { useTagStore } from '@/store/tagStore'
import { createHandler } from 'typesafe-agent-events'
import { fromDataWorker } from '@/worker/workerApi'
import { useLocationStore } from '@/store/locationStore'
import { useModalStore } from '@/store/modalStore'
import { useOptimisticStore } from '@/store/optimisticUpateStore'
import { useRedirectionStore } from '@/store/redirectionStore'
import { useTokenStore } from '@/store/tokenStore'
import { useChunkStore } from '@/store/chunkStore'

const workerHandlerMap = new Map<Worker, (e: MessageEvent) => void>()

/**
 * Establishes the event listeners for the DataWorker based on a specific isolation context.
 * Handles data synchronization, virtual scroll layout recalculations, and token management.
 *
 * @param dataWorker - The worker instance to attach listeners to.
 * @param isolationId - The unique context ID (e.g., tab or session) for store isolation.
 */
export function handleDataWorkerReturn(dataWorker: Worker, isolationId: IsolationId) {
  const messageStore = useMessageStore('mainId')
  const modalStore = useModalStore('mainId')
  const redirectionStore = useRedirectionStore('mainId')
  const tagStore = useTagStore('mainId')
  const tokenStore = useTokenStore(isolationId)
  const dataStore = useDataStore(isolationId)
  const prefetchStore = usePrefetchStore(isolationId)
  const locationStore = useLocationStore(isolationId)
  const optimisticUpdateStore = useOptimisticStore(isolationId)
  const chunkStore = useChunkStore(isolationId)

  const handler = createHandler<typeof fromDataWorker>({
    returnData: (payload) => {
      const slicedDataArray: SlicedData[] = payload.slicedDataArray
      slicedDataArray.forEach(({ index, data, hashToken }) => {
        dataStore.data.set(index, data)
        dataStore.hashMapData.set(data.id, index)

        // Business Logic: Albums rely on 'cover' for token validation,
        // whereas distinct media types (Images/Videos) use their unique ID.
        if (data.type === 'album') {
          if (data.cover !== null) {
            tokenStore.hashTokenMap.set(data.cover, hashToken)
          }
        } else {
          tokenStore.hashTokenMap.set(data.id, hashToken)
        }
      })
      dataStore.batchFetched.set(payload.batch, true)
      optimisticUpdateStore.selfUpdate()
    },

    fetchRowReturn: (payload) => {
      const { timestamp, chunkIndex, displayElements, start } = payload

      // Prevent updates if the view is locked (anchored) to a specific row to maintain scroll stability.
      if (locationStore.anchor !== null && locationStore.anchor !== chunkIndex) {
        return
      }

      const timestampMatched = timestamp === prefetchStore.timestamp

      if (timestampMatched) {
        chunkStore.setChunk(chunkIndex, displayElements)
      }

      // Second step of two-step locate jump: refine scroll to exact position.
      const pendingTarget = locationStore.pendingLocateTarget
      if (
        pendingTarget !== null &&
        pendingTarget >= start &&
        pendingTarget < start + displayElements.length
      ) {
        const elementIndex = pendingTarget - start
        if (elementIndex >= 0 && elementIndex < displayElements.length) {
          locationStore.highlightedIndex = pendingTarget
        }
        locationStore.pendingLocateTarget = null
      }

      prefetchStore.updateFetchRowTrigger = !prefetchStore.updateFetchRowTrigger
      prefetchStore.updateVisibleRowTrigger = !prefetchStore.updateVisibleRowTrigger
    },

    editTagsReturn: (payload) => {
      if (payload.returnedTagsArray !== undefined) {
        tagStore.applyTags(payload.returnedTagsArray)
      } else {
        console.warn('Returned tags array is undefined')
      }
      modalStore.showEditTagsModal = false
    },

    notification: (payload) => {
      messageStore.push(payload.text, payload.color)
    },

    unauthorized: async () => {
      await redirectionStore.redirectionToLogin()
    },

    refreshTimestampToken: (payload) => {
      tokenStore.timestampToken = payload.timestampToken
    },

    refreshHashToken: (payload) => {
      tokenStore.hashTokenMap.set(payload.hash, payload.hashToken)
    }
  })

  const messageHandler = (e: MessageEvent) => {
    handler(e.data as ReturnType<(typeof fromDataWorker)[keyof typeof fromDataWorker]>)
  }

  dataWorker.addEventListener('message', messageHandler)
  workerHandlerMap.set(dataWorker, messageHandler)
}

/**
 * Removes the message listener associated with the given DataWorker.
 * Used for cleanup to prevent memory leaks when components unmount.
 *
 * @param dataWorker - The worker instance to clean up.
 */
export function removeHandleDataWorkerReturn(dataWorker: Worker) {
  const messageHandler = workerHandlerMap.get(dataWorker)
  if (messageHandler) {
    dataWorker.removeEventListener('message', messageHandler)
    workerHandlerMap.delete(dataWorker)
  }
}
