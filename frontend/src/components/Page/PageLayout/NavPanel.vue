<template>
  <nav class="nav-panel no-select" :class="{ 'nav-panel--open': showNavPanel }">
    <div v-show="showNavPanel" class="nav-panel__inner">
      <v-list nav :key="route.fullPath" :disabled="!initializedStore.initialized">
        <v-list-item slim prepend-icon="mdi-home" title="Home" @click="goHome" />
        <v-list-item slim to="/albums" prepend-icon="mdi-image-album" title="Albums" />
        <v-list-item slim to="/tags" prepend-icon="mdi-tag-multiple" title="Tags" />
      </v-list>

      <v-list nav :key="route.fullPath" :disabled="!initializedStore.initialized" class="mt-auto">
        <v-divider />
        <v-list-item slim to="/trashed" prepend-icon="mdi-trash-can" title="Trashed" />
        <v-list-item slim to="/config" prepend-icon="mdi-tune" title="Config" />
      </v-list>
    </div>
  </nav>
</template>

<script setup lang="ts">
import { inject, type Ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useInitializedStore } from '@/store/initializedStore'

const showNavPanel = inject<Ref<boolean>>('showNavPanel')
const route = useRoute()
const router = useRouter()
const initializedStore = useInitializedStore('mainId')

const goHome = () => {
  window.scrollTo({ top: 0, behavior: 'smooth' })
  if (route.name !== 'timeline') {
    void router.push({ name: 'timeline' })
  }
}
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
