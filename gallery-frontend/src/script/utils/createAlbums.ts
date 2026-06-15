import axios from 'axios'
import { useAlbumStore } from '@/store/albumStore'
import { GalleryAlbum } from '@type/types'
import { useDataStore } from '@/store/dataStore'

export async function editTitle(album: GalleryAlbum, titleModelValue: string) {
  const albumStore = useAlbumStore('mainId')
  const dataStore = useDataStore('mainId')

  if ((album.title ?? '') !== titleModelValue) {
    const id = album.id
    const title = titleModelValue === '' ? null : titleModelValue
    await axios.put('/put/set_album_title', {
      albumId: id,
      title: title
    })
    const albumInfo = albumStore.albums.get(id)

    const index = dataStore.hashMapData.get(album.id)
    if (index !== undefined) {
      const data = dataStore.data.get(index)

      if (albumInfo && data?.type === 'album') {
        albumInfo.albumName = title
        albumInfo.displayName = albumInfo.albumName ?? 'Untitled'
        data.title = title
      } else {
        console.error(`Cannot find album with id ${id}`)
      }
    }
  }
}
