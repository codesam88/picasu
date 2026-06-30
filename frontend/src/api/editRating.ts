import axios from 'axios'
import { useDataStore } from '@/store/dataStore'
import { useMessageStore } from '@/store/messageStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { IsolationId } from '@/type/types'
import { tryWithMessageStore } from '@/script/utils/try_catch'

export async function editRating(
  rating: number | null,
  index: number,
  isolationId: IsolationId
): Promise<void> {
  const prefetchStore = usePrefetchStore(isolationId)
  const timestamp = prefetchStore.timestamp
  const messageStore = useMessageStore('mainId')
  const dataStore = useDataStore(isolationId)

  if (timestamp === null) {
    messageStore.error('Cannot set rating because timestamp is missing.')
    return
  }

  const item = dataStore.data.get(index)
  if (item) item.rating = rating

  await tryWithMessageStore('mainId', async () => {
    await axios.put('/put/edit_rating', {
      indexArray: [index],
      timestamp,
      rating
    })
    messageStore.success('Rating saved')
  })
}
