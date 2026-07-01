<template>
  <div class="pa-0 h-100 w-100 d-flex position-relative bg-background">
    <template v-if="index !== undefined">
      <ViewPageDisplay
        :abstract-data="abstractData"
        :index="index"
        :hash="hash"
        isolation-id="mainId"
      />
      <ViewPageMetadata
        v-if="abstractData && constStore.showInfo"
        :abstract-data="abstractData"
        :index="index"
        :hash="hash"
        isolation-id="mainId"
      />
    </template>
    <div
      v-else
      fluid
      class="pa-0 h-100 w-100 overflow-hidden position-relative"
      style="background-color: black"
    >
      <div class="d-flex align-center justify-center w-100 h-100">
        <v-progress-circular indeterminate color="primary" size="64" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useDataStore } from '@/store/dataStore'
import ViewPageDisplay from '@/components/View/Display/Display.vue'
import ViewPageMetadata from '@/components/View/Metadata/ViewPageMetadata.vue'
import { useConstStore } from '@/store/constStore'
import { useModalStore } from '@/store/modalStore'

const dataStore = useDataStore('mainId')
const route = useRoute()
const router = useRouter()
const constStore = useConstStore('mainId')
const modalStore = useModalStore('mainId')

const hash = computed(() => {
  return route.params.hash as string
})

const index = computed(() => {
  return dataStore.hashMapData.get(hash.value)
})

const abstractData = computed(() => {
  if (index.value !== undefined) {
    return dataStore.data.get(index.value)
  } else {
    return undefined
  }
})

// Escape closes an open dialog first; only when nothing is open does it act
// like a back button, returning to the page this view was entered from
// (rather than router.back(), which can land outside the app's own history
// or on a stale grid state depending on how this page was reached).
//
// Uses router.replace (not push): Escape undoes the "enter this view"
// navigation rather than taking a new forward step. push would leave the
// view page as a live history entry ahead of the grid, so the browser's own
// Back button would immediately re-enter the view you just escaped from.
const handleKeyDown = (event: KeyboardEvent) => {
  if (event.key !== 'Escape') return

  if (modalStore.hasOpenDialog) {
    modalStore.closeOpenDialog()
    return
  }

  const albumId = typeof route.params.albumId === 'string' ? route.params.albumId : undefined
  const shareId = typeof route.params.shareId === 'string' ? route.params.shareId : undefined
  const parentPage = route.meta.getParentPage(route, albumId, shareId)
  router.replace(parentPage).catch(() => {
    // No-op on navigation aborts (e.g. rapid double Escape).
  })
}

onMounted(() => {
  window.addEventListener('keydown', handleKeyDown)
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeyDown)
})
</script>
<style scoped>
.v-container::-webkit-scrollbar {
  display: none;
  /* Hide scrollbar */
}
</style>
