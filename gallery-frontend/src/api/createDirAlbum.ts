import { useAlbumStore } from '@/store/albumStore'
import { useMessageStore } from '@/store/messageStore'
import axios from 'axios'
import { tryWithMessageStore } from '@/script/utils/try_catch'

export async function createDirAlbum(
  parentAlbumId: string,
  name: string
): Promise<string | undefined> {
  const albumStore = useAlbumStore('mainId')
  const messageStore = useMessageStore('mainId')

  const newAlbumId = await tryWithMessageStore('mainId', async () => {
    const response = await axios.post<string>('/post/create_dir_album', { parentAlbumId, name })
    if (response.status !== 200) {
      throw new Error(`Server responded with status ${response.status}`)
    }
    await albumStore.fetchAlbums()
    messageStore.success(`Album "${name}" created.`)
    return response.data
  })

  return newAlbumId ?? undefined
}
