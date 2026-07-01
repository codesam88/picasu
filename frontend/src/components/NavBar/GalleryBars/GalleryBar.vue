<template>
  <GalleryBarTemplate isolation-id="mainId">
    <template #content>
      <v-toolbar v-if="!collectionStore.editModeOn" class="bg-surface">
        <v-btn v-if="route.meta.level === 1" @click="showDrawer = !showDrawer" icon="mdi-menu">
        </v-btn>
        <v-btn
          v-else
          icon="mdi mdi-arrow-left"
          :to="albumStore.leaveAlbumPath ? albumStore.leaveAlbumPath : '/'"
        ></v-btn>

        <v-card-title class="page-title text-truncate">
          {{ pageTitle }}
        </v-card-title>

        <v-card elevation="0" class="search-card">
          <v-card-text class="pa-0 bg-surface">
            <v-text-field
              id="nav-search-input"
              rounded
              class="ma-0"
              v-model="searchQuery"
              bg-color="surface-light"
              @click:prepend-inner="handleSearch"
              @click:clear="handleSearch"
              @keyup.enter="handleSearch"
              clearable
              persistent-clear
              variant="solo"
              flat
              prepend-inner-icon="mdi-magnify"
              single-line
              hide-details
              style="margin-right: 10px"
            >
              <template #label>
                <span class="text-body-small">Search</span>
              </template>
            </v-text-field>
          </v-card-text>
        </v-card>

        <v-btn
          v-if="route.meta.baseName === 'album'"
          icon="mdi-share-variant"
          @click="modalStore.showShareModal = true"
        />
        <v-btn
          v-if="route.meta.baseName === 'album'"
          icon="mdi-image-plus"
          @click="modalStore.showHomeTempModal = true"
        />
        <v-btn
          v-if="route.meta.level === 1"
          :icon="themeIsLight ? 'mdi-weather-sunny' : 'mdi-weather-night'"
          @click="themeIsLight = !themeIsLight"
        />
        <v-btn
          v-if="route.meta.level === 1"
          icon="mdi-upload"
          :loading="loading"
          @click="uploadStore.triggerFileInput(undefined)"
        />
      </v-toolbar>
      <EditBar v-else />

      <CreateShareModal
        v-if="
          modalStore.showShareModal &&
          route.meta.baseName === 'album' &&
          typeof route.params.hash === 'string'
        "
        :album-id="route.params.hash"
        :mode="'create'"
      />

      <GalleryTemp v-if="modalStore.showHomeTempModal && albumForTemp" :album="albumForTemp" />
    </template>
  </GalleryBarTemplate>
</template>

<script setup lang="ts">
import { computed, inject, Ref, ref, watchEffect } from 'vue'
import { LocationQueryValue, useRoute, useRouter } from 'vue-router'
import { useCollectionStore } from '@/store/collectionStore'
import { useFilterStore } from '@/store/filterStore'
import { useUploadStore } from '@/store/uploadStore'
import { useAlbumStore } from '@/store/albumStore'
import { useConstStore } from '@/store/constStore'
import { useModalStore } from '@/store/modalStore'
import EditBar from '@/components/NavBar/EditBar.vue'
import CreateShareModal from '@/components/Modal/CreateShareModal.vue'
import GalleryTemp from '@/components/Gallery/GalleryTemp.vue'
import { useTheme } from 'vuetify'
import GalleryBarTemplate from '@/components/NavBar/GalleryBars/GalleryBarTemplate.vue'
import { GalleryAlbum } from '@type/types'

const showDrawer = inject('showDrawer')

const albumStore = useAlbumStore('mainId')
const uploadStore = useUploadStore('mainId')
const filterStore = useFilterStore('mainId')
const constStore = useConstStore('mainId')
const modalStore = useModalStore('mainId')
const vuetifyTheme = useTheme()

const themeIsLight = computed<boolean>({
  get: () => constStore.theme === 'light',
  set: () => {
    constStore.toggleTheme(vuetifyTheme).catch((err: unknown) => {
      console.error('Failed to update theme (via InfoBar):', err)
    })
  }
})

const route = useRoute()
const router = useRouter()
const searchQuery: Ref<LocationQueryValue | LocationQueryValue[] | undefined> = ref(null)
const loading = ref(false)

const baseTitleMap: Record<string, string> = {
  timeline: 'Timeline',
  trashed: 'Trash',
  albums: 'Albums',
  album: 'Album',
  tags: 'Tags',
  config: 'Settings'
}

const pageTitle = computed(() => {
  const baseName = route.meta.baseName
  if (typeof baseName !== 'string') return ''
  if (baseName === 'album') {
    const id = route.params.hash
    if (typeof id !== 'string') return 'Album'
    const info = albumStore.albums.get(id)
    return info?.displayName ?? 'Album'
  }
  return baseTitleMap[baseName] ?? baseName
})

// Reconstructs a GalleryAlbum-shaped object from the lighter AlbumInfo store entry, so
// GalleryTemp (which expects a full UnifiedData album) can be opened from the persistent
// nav bar. albumHash identifies the current album at both level 1 and level 2.
const albumForTemp = computed((): GalleryAlbum | undefined => {
  if (route.meta.baseName !== 'album') return undefined
  const albumHash = route.params.albumHash
  if (typeof albumHash !== 'string') return undefined
  const info = albumStore.albums.get(albumHash)
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
    shareList: Object.fromEntries(info.shareList)
  }
})

const handleSearch = async () => {
  filterStore.searchString = searchQuery.value

  const nextQuery = { ...route.query }
  const v = searchQuery.value
  if (v === null || v === undefined || v === '') {
    delete nextQuery.search
  } else {
    nextQuery.search = v
  }

  await router.replace({
    path: route.path,
    query: nextQuery
  })
}

watchEffect(() => {
  searchQuery.value = filterStore.searchString
})

const collectionStore = useCollectionStore('mainId')
</script>

<style scoped>
.page-title {
  flex: 0 1 200px;
  min-width: 100px;
  font-size: 1.125rem;
  font-weight: 500;
  line-height: 1.175;
  letter-spacing: 0.0073529412em;
}

.search-card {
  flex: 1 1 auto;
  min-width: 200px;
}
</style>
