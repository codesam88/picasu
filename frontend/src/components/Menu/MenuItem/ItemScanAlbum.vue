<template>
  <v-list-item prepend-icon="mdi-folder-search-outline" @click="scan">
    <v-list-item-title class="wrap">Scan for New Files</v-list-item-title>
  </v-list-item>
</template>

<script lang="ts" setup>
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { startAlbumIndex, getAlbumIndexStatus } from '@/api/fs'
import { useAlbumStore } from '@/store/albumStore'
import { getIsolationIdByRoute } from '@utils/getter'

const route = useRoute()
const isolationId = getIsolationIdByRoute(route)
const albumStore = useAlbumStore(isolationId)
const messageStore = useMessageStore('mainId')

const scanPath = computed<string>(() => {
  if (route.meta.baseName === 'albums' && route.meta.level >= 2) {
    const albumId = route.params.hash
    if (typeof albumId === 'string') {
      const dirPath = albumStore.albums.get(albumId)?.dirPath
      if (dirPath !== null && dirPath !== undefined && dirPath !== '') return dirPath
    }
  }
  return '/'
})

const scan = async () => {
  await tryWithMessageStore('mainId', async () => {
    messageStore.info('Scanning for new files...')
    await startAlbumIndex(scanPath.value)
    let status = await getAlbumIndexStatus()
    while (status.state !== 'completed') {
      if (status.state === 'failed') throw new Error('Scan failed')
      if (status.state === 'canceled') throw new Error('Scan was canceled')
      await new Promise((resolve) => setTimeout(resolve, 500))
      status = await getAlbumIndexStatus()
    }
    messageStore.success('Scan complete')
  })
}
</script>
