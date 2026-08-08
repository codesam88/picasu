<template>
  <v-list-item prepend-icon="mdi-trash-can-outline" value="delete" @click="deleteData">
    <v-list-item-title class="wrap">Delete</v-list-item-title>
  </v-list-item>
</template>
<script lang="ts" setup>
import { useRoute } from 'vue-router'
import { getIsolationIdByRoute } from '@utils/getter'
import { useDataStore } from '@/store/dataStore'
import { usePrefetchStore } from '@/store/prefetchStore'
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

const deleteData = async () => {
  const timestamp = prefetchStore.timestamp
  if (timestamp === null) return

  const albumDir = typeof route.params.albumHash === 'string' ? route.params.albumHash : null

  const deleteList = props.indexList.map((index) => {
    const item = dataStore.data.get(index)
    const aliasPath =
      item !== undefined && 'alias' in item ? resolveAliasPath(item.alias, albumDir) : ''
    return { index, aliasPath }
  })

  await tryWithMessageStore('mainId', async () => {
    await axios.delete('/delete/delete-data', {
      data: { deleteList, timestamp }
    })
    messageStore.success('Successfully deleted data.')
  })
}
</script>
