<template>
  <v-list-item
    v-if="isAdmin"
    prepend-icon="mdi-share-variant"
    value="share-album"
    :disabled="!singleSelectedAlbumId"
    @click="openShare"
  >
    <v-list-item-title class="wrap">Share album</v-list-item-title>
  </v-list-item>
</template>
<script lang="ts" setup>
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useModalStore } from '@/store/modalStore'
import { useCollectionStore } from '@/store/collectionStore'
import { useDataStore } from '@/store/dataStore'
import { canSeeUserPanel, useCurrentUser } from '@/script/utils/currentUser'
import { getIsolationIdByRoute } from '@utils/getter'

const route = useRoute()
const isolationId = getIsolationIdByRoute(route)
const modalStore = useModalStore('mainId')
const collectionStore = useCollectionStore(isolationId)
const dataStore = useDataStore(isolationId)
const me = useCurrentUser()
const isAdmin = computed(() => canSeeUserPanel(me.value))

// Only enabled when exactly one selected grid item is itself an album — the
// only case a "Share album" action on a selection makes sense. Mirrors the
// Album Info entry's gating.
const singleSelectedAlbumId = computed<string | null>(() => {
  if (collectionStore.editModeCollection.size !== 1) return null
  const index = Array.from(collectionStore.editModeCollection)[0]
  if (index === undefined) return null
  const item = dataStore.data.get(index)
  return item?.type === 'album' ? item.id : null
})

const openShare = () => {
  const albumId = singleSelectedAlbumId.value
  if (albumId === null) return
  modalStore.shareAlbumId = albumId
  modalStore.showShareModal = true
}
</script>
