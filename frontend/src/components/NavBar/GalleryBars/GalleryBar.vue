<template>
  <GalleryBarTemplate isolation-id="mainId">
    <template #content>
      <v-toolbar v-if="!collectionStore.editModeOn" class="bg-surface">
        <v-tooltip v-if="route.meta.level === 1" location="top" text="Menu">
          <template #activator="{ props }">
            <v-btn v-bind="props" @click="showNavPanel = !showNavPanel" icon="mdi-menu" />
          </template>
        </v-tooltip>
        <v-tooltip v-else location="top" text="Back">
          <template #activator="{ props }">
            <v-btn v-bind="props" icon="mdi mdi-arrow-left" @click="leaveView(route, router)" />
          </template>
        </v-tooltip>

        <template v-if="route.meta.baseName === 'album'">
          <v-card-title class="page-title">Albums</v-card-title>
          <v-breadcrumbs v-if="breadcrumbs.length > 0" class="pa-0" density="compact">
            <template v-for="(crumb, i) in breadcrumbs" :key="crumb.id">
              <v-breadcrumbs-divider class="mx-1">/</v-breadcrumbs-divider>
              <v-breadcrumbs-item
                :disabled="i === breadcrumbs.length - 1"
                @click="i < breadcrumbs.length - 1 && navigateToCrumb(crumb)"
                class="text-body-1"
              >
                {{ crumb.name }}
              </v-breadcrumbs-item>
            </template>
          </v-breadcrumbs>
        </template>
        <v-card-title v-else class="page-title text-truncate">
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

        <v-tooltip v-if="route.meta.baseName === 'album'" location="top" text="Share">
          <template #activator="{ props }">
            <v-btn
              v-bind="props"
              icon="mdi-share-variant"
              @click="modalStore.showShareModal = true"
            />
          </template>
        </v-tooltip>
        <v-tooltip v-if="route.meta.level === 1" location="top" text="Theme">
          <template #activator="{ props }">
            <v-btn
              v-bind="props"
              :icon="themeIsLight ? 'mdi-weather-sunny' : 'mdi-weather-night'"
              @click="themeIsLight = !themeIsLight"
            />
          </template>
        </v-tooltip>
        <v-tooltip v-if="route.meta.level === 1" location="top" text="Upload">
          <template #activator="{ props }">
            <v-btn
              v-bind="props"
              icon="mdi-upload"
              :loading="loading"
              @click="uploadStore.triggerFileInput(undefined)"
            />
          </template>
        </v-tooltip>
        <v-menu v-if="route.meta.level === 1" location="bottom end">
          <template #activator="{ props }">
            <v-btn v-bind="props" icon="mdi-account-circle" />
          </template>
          <v-list density="compact" class="pa-1">
            <v-list-item
              prepend-icon="mdi-cog"
              title="Settings"
              @click="modalStore.showUserSettingsModal = true"
            />
            <v-list-item prepend-icon="mdi-logout" title="Log out" @click="handleLogout" />
          </v-list>
        </v-menu>
      </v-toolbar>
      <EditBar v-else />

      <CreateShareModal
        v-if="
          modalStore.showShareModal &&
          route.meta.baseName === 'album' &&
          typeof route.params.albumHash === 'string'
        "
        :album-id="route.params.albumHash"
        :mode="'create'"
      />
    </template>
  </GalleryBarTemplate>
</template>

<script setup lang="ts">
import { computed, inject, Ref, ref, watchEffect } from 'vue'
import Cookies from 'js-cookie'
import { LocationQueryValue, useRoute, useRouter } from 'vue-router'
import { useCollectionStore } from '@/store/collectionStore'
import { useFilterStore } from '@/store/filterStore'
import { useUploadStore } from '@/store/uploadStore'
import { useAlbumStore } from '@/store/albumStore'
import { useConstStore } from '@/store/constStore'
import { useModalStore } from '@/store/modalStore'
import EditBar from '@/components/NavBar/EditBar.vue'
import CreateShareModal from '@/components/Modal/CreateShareModal.vue'
import { useTheme } from 'vuetify'
import GalleryBarTemplate from '@/components/NavBar/GalleryBars/GalleryBarTemplate.vue'
import { leaveView } from '@utils/leaveView'

const showNavPanel = inject('showNavPanel')

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

interface Breadcrumb {
  name: string
  id: string
}

const breadcrumbs = computed<Breadcrumb[]>(() => {
  if (route.meta.baseName !== 'album') return []

  const albumHash = route.params.albumHash
  if (typeof albumHash !== 'string') return []

  const trail: Breadcrumb[] = []
  let currentId: string | null = albumHash

  while (currentId !== null) {
    const info = albumStore.albums.get(currentId)
    if (!info) break
    trail.unshift({ name: info.displayName, id: currentId })
    currentId = info.parentAlbumId ?? null
  }

  if (trail.length > 4) {
    return trail.slice(trail.length - 4)
  }

  return trail
})

const navigateToCrumb = (crumb: Breadcrumb) => {
  void router.push({ name: 'album', params: { albumHash: crumb.id } })
}

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
    const albumHash = route.params.albumHash
    if (typeof albumHash !== 'string') return 'Album'
    const info = albumStore.albums.get(albumHash)
    return info?.displayName ?? 'Album'
  }
  return baseTitleMap[baseName] ?? baseName
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

const handleLogout = async () => {
  Cookies.remove('jwt')
  await router.push({ name: 'login' })
}

watchEffect(() => {
  searchQuery.value = filterStore.searchString
})

const collectionStore = useCollectionStore('mainId')
</script>

<style scoped>
.page-title {
  flex: 0 1 auto;
  min-width: 100px;
  font-size: 1.125rem;
  font-weight: 500;
  line-height: 1.175;
  letter-spacing: 0.0073529412em;
}

.search-card {
  flex: 1 1 auto;
}
</style>
