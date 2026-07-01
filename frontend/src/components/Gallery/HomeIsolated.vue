<template>
  <v-overlay
    v-model="overlayVisible"
    :height="'100%'"
    :width="'100%'"
    class="d-flex"
    id="view-page"
    :transition="false"
    :close-on-back="false"
  >
    <Gallery
      v-if="album !== undefined && basicString !== null"
      :key="isolatedHomeKey"
      isolation-id="subId"
      :basic-string="basicString"
      :search-string="searchString"
    >
      <template #home-toolbar>
        <HomeIsolatedBar :album="album" />
      </template>
    </Gallery>
  </v-overlay>
</template>
<script setup lang="ts">
import Gallery from './Gallery.vue'
import HomeIsolatedBar from '@/components/NavBar/GalleryBars/HomeIsolatedBar.vue'
import { GalleryAlbum } from '@type/types'
import { computed, Ref, ref, watch } from 'vue'
import { useCollectionStore } from '@/store/collectionStore'
import { LocationQueryValue, useRoute, useRouter } from 'vue-router'
import { useDataStore } from '@/store/dataStore'
import { useAlbumStore } from '@/store/albumStore'

const route = useRoute()
const router = useRouter()
const dataStore = useDataStore('mainId')
const albumStore = useAlbumStore('mainId')
const album: Ref<GalleryAlbum | undefined> = ref(undefined)

const basicString = computed<string | null>(() => {
  const id = route.params.hash
  if (typeof id !== 'string') return null
  return `and(trashed:false, or(album:"${id}", parent_album:"${id}"))`
})

const searchString = ref<LocationQueryValue | LocationQueryValue[] | undefined>(null)
const collectionStore = useCollectionStore('subId')

const overlayVisible = computed<boolean>({
  get() {
    return true
  },
  set(val: boolean) {
    if (!val) {
      if (collectionStore.editModeOn) {
        collectionStore.editModeOn = false
      } else {
        router.back()
      }
    }
  }
})

const subSearchKey = computed(() => {
  const v = route.query.subSearch
  if (typeof v === 'string') return v
  if (Array.isArray(v)) return v.join(',')
  return ''
})

const locateKey = computed(() => {
  const v = route.query.locate
  if (typeof v === 'string') return v
  if (Array.isArray(v)) return v.join(',')
  return ''
})

const priorityKey = computed(() => {
  const v = route.query.priority_id
  if (typeof v === 'string') return v
  if (Array.isArray(v)) return v.join(',')
  return ''
})

const reverseKey = computed(() => {
  const v = route.query.reverse
  if (typeof v === 'string') return v
  if (Array.isArray(v)) return v.join(',')
  return ''
})

const hashKey = computed(() => (typeof route.params.hash === 'string' ? route.params.hash : ''))

// This forces ONLY the isolated Home to remount when subSearch changes
const isolatedHomeKey = computed(() => {
  return `isolated-${hashKey.value}-${subSearchKey.value}-${locateKey.value}-${priorityKey.value}-${reverseKey.value}`
})

watch(
  () => route.query.subSearch,
  (v) => {
    searchString.value = v
  },
  { immediate: true }
)

function albumFromStore(id: string): GalleryAlbum | undefined {
  const info = albumStore.albums.get(id)
  if (info === undefined) return undefined
  return {
    type: 'album' as const,
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
    shareList: info.shareList
  }
}

watch(
  () => route.params.hash,
  (hash) => {
    album.value = undefined
    if (typeof hash !== 'string') return
    const index = dataStore.hashMapData.get(hash)
    if (index !== undefined) {
      const data = dataStore.data.get(index)
      if (data?.type === 'album') {
        album.value = data
        return
      }
    }
    album.value = albumFromStore(hash)
    if (album.value === undefined && !albumStore.fetched) {
      void albumStore.fetchAlbums().then(() => {
        if (route.params.hash === hash) {
          album.value ??= albumFromStore(hash)
        }
      })
    }
  },
  { immediate: true }
)
</script>

<style scoped></style>
