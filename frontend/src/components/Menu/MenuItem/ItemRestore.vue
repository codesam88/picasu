<template>
  <v-list-item prepend-icon="mdi-restore" value="restore" @click="restoreData">
    <v-list-item-title class="wrap">Restore</v-list-item-title>
  </v-list-item>
</template>
<script lang="ts" setup>
import { useRoute } from 'vue-router'
import { getIsolationIdByRoute } from '@utils/getter'
import { useDataStore } from '@/store/dataStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { refreshGalleryAfterMutation } from '@/script/hook/usePrefetch'
import { Alias } from '@type/types'
import axios from 'axios'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'

const props = defineProps<{
  indexList: number[]
}>()

const route = useRoute()
const isolationId = getIsolationIdByRoute(route)
const dataStore = useDataStore(isolationId)
const prefetchStore = usePrefetchStore(isolationId)
const messageStore = useMessageStore('mainId')

function resolveAliasPath(alias: Alias[] | undefined, albumDir: string | null): string {
  if (alias === undefined || alias.length === 0) return ''
  if (albumDir !== null && albumDir !== '') {
    const match = alias.find((a) => a.file.startsWith(albumDir))
    if (match !== undefined) return match.file
  }
  return alias[0]?.file ?? ''
}

const restoreData = async () => {
  const timestamp = prefetchStore.timestamp
  if (timestamp === null) return

  const albumDir = typeof route.params.albumHash === 'string' ? route.params.albumHash : null

  const restoreList = props.indexList.map((index) => {
    const item = dataStore.data.get(index)
    const aliasPath =
      item !== undefined && 'alias' in item ? resolveAliasPath(item.alias, albumDir) : ''
    return { index, aliasPath }
  })

  await tryWithMessageStore('mainId', async () => {
    await axios.put('/put/restore-from-trash', {
      restoreList,
      timestamp
    })
    messageStore.success('Successfully restored data.')
    await refreshGalleryAfterMutation(isolationId, route)
  })
}
</script>
