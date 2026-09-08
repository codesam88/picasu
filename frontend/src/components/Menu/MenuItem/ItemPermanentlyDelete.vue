<template>
  <v-list-item prepend-icon="mdi-trash-can-outline" value="permanently-delete" @click="deleteData">
    <v-list-item-title class="wrap">Permanently Delete</v-list-item-title>
  </v-list-item>
</template>

<script lang="ts" setup>
import { useRoute } from 'vue-router'
import { getIsolationIdByRoute } from '@utils/getter'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useDataStore } from '@/store/dataStore'
import axios from 'axios'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
const route = useRoute()
const isolationId = getIsolationIdByRoute(route)
const prefetchStore = usePrefetchStore(isolationId)
const dataStore = useDataStore(isolationId)
const messageStore = useMessageStore('mainId')
const props = defineProps<{
  indexList: number[]
}>()

const deleteData = async () => {
  const timestamp = prefetchStore.timestamp
  if (timestamp === null) return

  await tryWithMessageStore('mainId', async () => {
    const aliasList = props.indexList.map((index) => {
      const item = dataStore.data.get(index)
      if (item && (item.type === 'image' || item.type === 'video')) {
        return item.alias[0]?.file ?? null
      }
      return null
    })

    await axios.delete('/delete/delete-data', {
      data: { deleteList: props.indexList, aliasList, timestamp }
    })
    messageStore.success('Successfully deleted data.')
  })
}
</script>
