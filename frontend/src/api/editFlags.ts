import { useMessageStore } from '@/store/messageStore'
import { useDataStore } from '@/store/dataStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { IsolationId } from '@/type/types'
import axios from 'axios'
import { tryWithMessageStore } from '@/script/utils/try_catch'

export interface EditFlagsPayload {
  indexArray: number[]
  timestamp: number
  isTrashed?: boolean
}

/**
 * Update the boolean flag (isTrashed) on one or more items.
 *
 * This is the dedicated API for flag mutations, separate from `editTags` which
 * handles string tags. Favorite and archived were removed together with the
 * fields they toggled, so trash is the only flag this endpoint carries.
 */
export async function editFlags(
  indexArray: number[],
  flags: { isTrashed?: boolean },
  isolationId: IsolationId
) {
  const prefetchStore = usePrefetchStore(isolationId)
  const timestamp = prefetchStore.timestamp
  const messageStore = useMessageStore('mainId')
  const dataStore = useDataStore(isolationId)

  if (timestamp === null) {
    messageStore.error('Cannot edit flags because timestamp is missing.')
    return
  }

  // Optimistic update
  for (const index of indexArray) {
    const data = dataStore.data.get(index)
    if (data) {
      if (flags.isTrashed !== undefined) {
        const isTrashed = flags.isTrashed
        data.isTrashed = isTrashed
        if ((data.type === 'image' || data.type === 'video') && data.path) {
          data.path.isTrashed = isTrashed
        }
      }
    }
  }

  await tryWithMessageStore('mainId', async () => {
    await axios.put('/put/edit_flags', {
      indexArray,
      timestamp,
      ...flags
    })

    messageStore.success('Successfully updated.')
  })
}

// Convenience functions
export async function setTrashed(indexArray: number[], value: boolean, isolationId: IsolationId) {
  await editFlags(indexArray, { isTrashed: value }, isolationId)
}
