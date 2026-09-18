import { useMessageStore } from '@/store/messageStore'
import { useDataStore } from '@/store/dataStore'
import { IsolationId } from '@/type/types'
import axios from 'axios'
import { tryWithMessageStore } from '@/script/utils/try_catch'

export type AssignOutcome = 'moved' | 'renamedFrom' | 'skipped'

interface AssignAlbumResult {
  outcome: AssignOutcome
}

export async function assignAlbum(
  hash: string,
  albumId: string,
  index: number,
  isolationId: IsolationId,
  onConflict: 'skip' | 'rename'
): Promise<boolean> {
  const messageStore = useMessageStore('mainId')
  const dataStore = useDataStore(isolationId)

  const success = await tryWithMessageStore('mainId', async () => {
    const item = dataStore.data.get(index)
    const body: { hash: string; albumId: string; onConflict: 'skip' | 'rename'; alias?: string } = {
      hash,
      albumId,
      onConflict
    }
    if (item !== undefined && item.type !== 'album') {
      const alias = item.alias[0]?.file
      if (alias !== undefined) body.alias = alias
    }
    const response = await axios.put<AssignAlbumResult>('/put/assign_album', body)
    if (response.status !== 200) {
      throw new Error(`Server responded with status ${response.status}`)
    }
    dataStore.setAlbum(index, albumId)
    if (response.data.outcome === 'renamedFrom') {
      messageStore.success('Moved to album (file renamed).')
    } else if (response.data.outcome === 'skipped') {
      messageStore.info('File already exists in album; skipped.')
    } else {
      messageStore.success('Moved to album.')
    }
    return true
  })

  return success === true
}
