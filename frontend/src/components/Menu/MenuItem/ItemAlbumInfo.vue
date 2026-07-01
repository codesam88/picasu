<template>
  <v-list-item
    prepend-icon="mdi-information-outline"
    value="album-info"
    :disabled="!isSingleAlbumSelected"
    @click="modalStore.showAlbumInfoModal = true"
  >
    <v-list-item-title class="wrap">Album Info</v-list-item-title>
  </v-list-item>
</template>

<script lang="ts" setup>
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useModalStore } from '@/store/modalStore'
import { useCollectionStore } from '@/store/collectionStore'
import { useDataStore } from '@/store/dataStore'
import { getIsolationIdByRoute } from '@utils/getter'

const route = useRoute()
const isolationId = getIsolationIdByRoute(route)
const modalStore = useModalStore('mainId')
const collectionStore = useCollectionStore(isolationId)
const dataStore = useDataStore(isolationId)

// Only enabled when exactly one selected grid item is itself an album — the
// only case an "Album Info" action on a selection makes sense.
const isSingleAlbumSelected = computed<boolean>(() => {
  if (collectionStore.editModeCollection.size !== 1) return false
  const index = Array.from(collectionStore.editModeCollection)[0]
  if (index === undefined) return false
  return dataStore.data.get(index)?.type === 'album'
})
</script>
