<template>
  <v-list-item prepend-icon="mdi-image-refresh-outline" @click="regenerateThumbnailByFrame">
    <v-list-item-title class="wrap">Capture Frame as Thumb</v-list-item-title>
  </v-list-item>
</template>

<script lang="ts" setup>
import { useRoute } from 'vue-router'
import axios from 'axios'
import { getIsolationIdByRoute } from '@utils/getter'
import { useCurrentFrameStore } from '@/store/currentFrameStore'
import { useMessageStore } from '@/store/messageStore'
import { useEditStore } from '@/store/editStore'
import { useDataStore } from '@/store/dataStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'

const route = useRoute()
const isolationId = getIsolationIdByRoute(route)
const currentFrameStore = useCurrentFrameStore(isolationId)
const messageStore = useMessageStore('mainId')
const editStore = useEditStore('mainId')
const dataStore = useDataStore(isolationId)

const regenerateThumbnailByFrame = async () => {
  const hash = route.params.hash
  if (typeof hash !== 'string') return

  // Resolve assetId from data store
  const index = dataStore.hashMapData.get(hash)
  const data = index !== undefined ? dataStore.data.get(index) : undefined
  const assetId = data?.assetId
  if (assetId === undefined) {
    messageStore.error('Item has no assetId; cannot regenerate')
    return
  }

  if (editStore.hasRegenerate(assetId)) return

  editStore.addRegenerate(assetId)
  try {
    await tryWithMessageStore(isolationId, async () => {
      const currentFrameBlob = await currentFrameStore.getCapture()
      if (currentFrameBlob) {
        const formData = new FormData()

        // Append the hash for backend compatibility
        formData.append('hash', hash)

        // Append the frame file
        formData.append('frame', currentFrameBlob)
        messageStore.info('Regenerating thumbnail...')

        const response = await axios.put('/put/regenerate-thumbnail-with-frame', formData, {
          headers: {
            'Content-Type': 'multipart/form-data'
          }
        })

        messageStore.success('Regenerating thumbnail successfully')
        console.log('Response:', response.data)
      }
    })
  } finally {
    editStore.removeRegenerate(assetId)
  }
}
</script>
