import { AlbumInfo, IsolationId } from '@type/types'
import { albumInfoSchema } from '@type/schemas'
import axios from 'axios'
import { defineStore } from 'pinia'
import { useSessionStore } from '@/store/sessionStore'
import { z } from 'zod'
import { tryWithMessageStore } from '@/script/utils/try_catch'

export const useAlbumStore = (isolationId: IsolationId) =>
  defineStore('albumStore' + isolationId, {
    state: (): {
      albums: Map<string, AlbumInfo> // id -> album
      fetched: boolean
    } => ({
      albums: new Map(),
      fetched: false
    }),
    actions: {
      async fetchAlbums() {
        await tryWithMessageStore('mainId', async () => {
          const response = await axios.get('/get/get-albums')

          if (response.status !== 200) {
            throw new Error('Network response was not ok')
          }

          const albums = z.array(albumInfoSchema).parse(response.data)

          const currentUserId = useSessionStore('mainId').profile?.userId ?? null
          albums.forEach((album) => {
            // Defensive: never retain another user's personal album (the
            // server already filters; this is belt-and-braces).
            if (album.owner != null && album.owner !== currentUserId) return
            this.albums.set(album.albumId, album)
          })

          this.fetched = true
        })
      },
      clearAll() {
        this.albums.clear()
        this.fetched = false
      }
    }
  })()
