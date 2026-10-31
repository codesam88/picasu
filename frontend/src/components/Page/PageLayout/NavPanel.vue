<template>
  <nav class="nav-panel no-select" :class="{ 'nav-panel--open': showNavPanel }">
    <div v-show="showNavPanel" class="nav-panel__inner">
      <v-list nav :key="route.fullPath" :disabled="!initializedStore.initialized">
        <v-list-item slim prepend-icon="mdi-home" title="Home" :to="{ name: 'timeline' }" />
        <v-list-item slim prepend-icon="mdi-image-album" title="Albums" to="/albums" />
        <v-list-item
          v-if="showMyAlbums"
          slim
          prepend-icon="mdi-account-circle"
          title="My Albums"
          to="/my-albums"
        />
        <v-list-item slim prepend-icon="mdi-tag-multiple" title="Tags" to="/tags" />
        <v-list-item slim prepend-icon="mdi-share-variant" title="Shares" to="/shares" />
      </v-list>

      <v-list nav :key="route.fullPath" :disabled="!initializedStore.initialized" class="mt-auto">
        <v-divider />
        <v-list-item
          v-if="isAdmin"
          slim
          prepend-icon="mdi-shield-crown"
          title="Admin"
          to="/config"
        />
        <v-list-item slim prepend-icon="mdi-trash-can" title="Trashed" to="/trashed" />
      </v-list>
    </div>
  </nav>
</template>

<script setup lang="ts">
import { computed, inject, type Ref } from 'vue'
import { useRoute } from 'vue-router'
import { useInitializedStore } from '@/store/initializedStore'
import { useAlbumStore } from '@/store/albumStore'
import { onMounted } from 'vue'
import { useCurrentUser, canSeeUserPanel } from '@/script/utils/currentUser'

const showNavPanel = inject<Ref<boolean>>('showNavPanel')
const route = useRoute()
const initializedStore = useInitializedStore('mainId')
const currentUser = useCurrentUser()
const albumStore = useAlbumStore('mainId')
const isAdmin = computed(() => canSeeUserPanel(currentUser.value))

const showMyAlbums = computed(() => {
  if (currentUser.value === null) return false
  // Personal areas are enabled iff the caller-visible album list contains at
  // least one album in the personal root. Fields are optional/legacy: an
  // album without `root` never matches, so the entry stays hidden.
  return Array.from(albumStore.albums.values()).some((a) => a.root === 'personal')
})

onMounted(() => {
  if (!albumStore.fetched) {
    void albumStore.fetchAlbums()
  }
})
</script>

<style scoped>
.nav-panel {
  width: 0;
  flex-shrink: 0;
  transition: width 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  overflow: hidden;
}

.nav-panel--open {
  width: 150px;
}

.nav-panel__inner {
  width: 150px;
  height: 100%;
  display: flex;
  flex-direction: column;
  background-color: rgb(var(--v-theme-surface));
  border-right: thin solid rgba(var(--v-theme-on-surface), 0.12);
  overflow-y: auto;
}
</style>
