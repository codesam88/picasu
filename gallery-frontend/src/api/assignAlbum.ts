import { useMessageStore } from '@/store/messageStore'
import { useDataStore } from '@/store/dataStore'
import { IsolationId } from '@/type/types'
import axios from 'axios'
import { tryWithMessageStore } from '@/script/utils/try_catch'

export async function assignAlbum(
  hash: string,
  albumId: string,
  index: number,
  isolationId: IsolationId
): Promise<boolean> {
  const messageStore = useMessageStore('mainId')
  const dataStore = useDataStore(isolationId)

  const success = await tryWithMessageStore('mainId', async () => {
    const response = await axios.put('/put/assign_album', { hash, albumId })
    if (response.status !== 200) {
      throw new Error(`Server responded with status ${response.status}`)
    }
    dataStore.setAlbum(index, albumId)
    messageStore.success('Moved to album.')
    return true
  })

  return success === true
}
