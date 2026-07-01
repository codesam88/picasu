<template>
  <v-overlay
    v-model="modalStore.showHomeTempModal"
    :height="'100%'"
    :width="'100%'"
    class="d-flex"
    id="view-page"
    :transition="false"
    :close-on-back="false"
  >
    <Gallery isolation-id="tempId" :basic-string="basicString" :search-string="null">
      <template #home-toolbar>
        <GalleryTempBar :album="album" />
      </template>
    </Gallery>
  </v-overlay>
</template>
<script setup lang="ts">
import { GalleryAlbum } from '@type/types'
import Gallery from './Gallery.vue'
import GalleryTempBar from '@/components/NavBar/GalleryBars/GalleryTempBar.vue'
import { useModalStore } from '@/store/modalStore'
import { onBeforeRouteLeave } from 'vue-router'
const modalStore = useModalStore('mainId')
const props = defineProps<{
  album: GalleryAlbum
}>()

const basicString = `and(not(type:"album"), trashed:false, not(album:"${props.album.id}"))`
onBeforeRouteLeave(() => {
  if (modalStore.showHomeTempModal) {
    modalStore.showHomeTempModal = false
    return false
  }
})
</script>
