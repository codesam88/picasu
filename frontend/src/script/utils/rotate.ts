import axios from 'axios'
import { useDataStore } from '@/store/dataStore'
import { useEditStore } from '@/store/editStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { IsolationId } from '@/type/types'

/**
 * Common handler for rotating an image.
 * This logic is shared between the UI menu item and keyboard shortcuts.
 */
export const handleRotateImage = async (
  assetId: string,
  isolationId: IsolationId
): Promise<void> => {
  const editStore = useEditStore('mainId')

  // 1. Optimistic Update (Immediate visual feedback)
  editStore.incrementRotation(assetId)

  // 2. Queue Backend Request (Serialized execution)
  // We use the queue to ensure requests are processed in order (1 -> 2 -> 3)
  // even if the user clicks rapidly. This prevents race conditions on the backend.
  await editStore.queueRotate(assetId, async () => {
    await tryWithMessageStore(isolationId, async () => {
      await axios.put('/put/rotate-image', { assetId })
      // Mirror the server-side swap locally so viewer metadata updates
      // immediately instead of waiting for the next prefetch. The backend
      // rewrote the compressed bytes and bumped its timestamp; dating the
      // local record now busts thumbnail caches keyed on updatedAt.
      const dataStore = useDataStore(isolationId)
      const index = dataStore.assetIdMapData.get(assetId)
      let record: ReturnType<typeof dataStore.data.get>
      if (index === undefined) {
        record = undefined
      } else {
        record = dataStore.data.get(index)
      }
      if (record?.type === 'image') {
        const width = record.width
        record.width = record.height
        record.height = width
        record.updateAt = Date.now()
      }
      return true
    })
  })
}
