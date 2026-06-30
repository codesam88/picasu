<template>
  <v-overlay
    v-model="overlayVisible"
    height="100%"
    width="100%"
    class="d-flex"
    id="view-page"
    :transition="false"
    :close-on-back="false"
  >
    <div v-if="index !== undefined" class="pa-0 h-100 w-100 d-flex position-relative bg-background">
      <ViewPageDisplay
        :abstract-data="abstractData"
        :index="index"
        :hash="hash"
        :isolation-id="isolationId"
      />
      <ViewPageMetadata
        v-if="abstractData && constStore.showInfo"
        :abstract-data="abstractData"
        :index="index"
        :hash="hash"
        :isolation-id="isolationId"
      />
    </div>
    <div
      v-else-if="albumFallback !== undefined"
      class="pa-0 h-100 w-100 d-flex position-relative bg-background"
    >
      <ViewPageDisplay
        :abstract-data="albumFallback"
        :index="0"
        :hash="hash"
        :isolation-id="isolationId"
      />
    </div>
    <div
      v-else
      fluid
      class="pa-0 h-100 overflow-hidden position-relative"
      style="background-color: black"
    >
      <div class="d-flex align-center justify-center w-100 h-100">
        <v-progress-circular indeterminate color="primary" size="64" />
      </div>
    </div>
  </v-overlay>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useDataStore } from '@/store/dataStore'
import { useAlbumStore } from '@/store/albumStore'
import ViewPageDisplay from '@/components/View/Display/Display.vue'
import ViewPageMetadata from '@/components/View/Metadata/ViewPageMetadata.vue'
import { IsolationId, EnrichedUnifiedData } from '@type/types'
import { useConstStore } from '@/store/constStore'
const props = defineProps<{
  isolationId: IsolationId
}>()

const dataStore = useDataStore(props.isolationId)
const albumStore = useAlbumStore('mainId')
const route = useRoute()
const router = useRouter()
const constStore = useConstStore('mainId')

const overlayVisible = computed<boolean>({
  get() {
    // The overlay is always visible as long as this component exists.
    return true
  },
  set(val: boolean) {
    if (!val) {
      // When the overlay is requested to close (e.g., via ESC), navigate back.
      router.back()
    }
  }
})

const hash = computed(() => {
  if (props.isolationId === 'mainId') {
    return route.params.hash as string
  } else {
    return route.params.subhash as string
  }
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

// Sub-albums are not in the main page's dataStore (only root albums are loaded there).
// When index is undefined and isolationId is mainId, try albumStore so DisplayAlbum.vue
// can mount and provide its router-view for HomeIsolated at level 3.
const albumFallback = computed((): EnrichedUnifiedData | undefined => {
  if (index.value !== undefined || props.isolationId !== 'mainId') return undefined
  const info = albumStore.albums.get(hash.value)
  if (!info) return undefined
  return {
    type: 'album',
    id: info.albumId,
    title: info.albumName,
    startTime: null,
    endTime: null,
    lastModifiedTime: 0,
    cover: null,
    thumbhash: null,
    tags: [],
    itemCount: 0,
    itemSize: 0,
    pending: false,
    description: null,
    isFavorite: false,
    isArchived: false,
    isTrashed: false,
    rating: null,
    updateAt: 0,
    shareList: Object.fromEntries(info.shareList),
    thumbhashUrl: null,
    timestamp: 0
  }
})
</script>
<style scoped>
.v-container::-webkit-scrollbar {
  display: none;
  /* Hide scrollbar */
}
</style>
