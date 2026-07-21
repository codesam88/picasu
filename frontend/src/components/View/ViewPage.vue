<template>
  <div class="pa-0 h-100 w-100 position-relative bg-background">
    <template v-if="index !== undefined">
      <div
        class="view-stage"
        :class="{ 'view-stage--desktop': !configStore.isMobile }"
        @click.self="handleClose"
      >
        <div class="view-container">
          <div class="view-modal">
            <NavigationOverlays
              v-if="!configStore.isMobile"
              :previous-hash="previousHash"
              :next-hash="nextHash"
              :previous-page="previousPage"
              :next-page="nextPage"
              :show="!configStore.isMobile"
            />
            <div class="view-image-container">
              <ViewPageDisplay
                :abstract-data="abstractData"
                :index="index"
                :hash="hash"
                isolation-id="mainId"
              />
            </div>
          </div>
          <ViewPageMetadata
            v-if="showMetadataPanel && abstractData"
            class="view-modal-sidepane"
            :abstract-data="abstractData"
            :index="index"
            :hash="hash"
            isolation-id="mainId"
          />
          <div class="view-modal-controls">
            <v-tooltip location="top" text="Info">
              <template #activator="{ props }">
                <v-btn
                  v-bind="props"
                  icon
                  size="small"
                  variant="text"
                  class="control-btn"
                  aria-label="Info"
                  @click="toggleMetadataPanel"
                >
                  <v-icon>{{
                    showMetadataPanel ? 'mdi-information' : 'mdi-information-outline'
                  }}</v-icon>
                </v-btn>
              </template>
            </v-tooltip>
            <DatabaseMenu
              v-if="
                abstractData &&
                (abstractData.type === 'image' || abstractData.type === 'video') &&
                route.meta.baseName !== 'share' &&
                share === null
              "
              :database="abstractData"
              :index="index"
              :hash="hash"
              isolation-id="mainId"
            />
            <v-tooltip location="top" text="Close">
              <template #activator="{ props }">
                <v-btn
                  v-bind="props"
                  icon
                  size="small"
                  variant="text"
                  class="control-btn"
                  aria-label="Close"
                  @click="handleClose"
                >
                  <v-icon>mdi-close</v-icon>
                </v-btn>
              </template>
            </v-tooltip>
          </div>
        </div>
      </div>
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
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useDataStore } from '@/store/dataStore'
import { useConfigStore } from '@/store/configStore'
import ViewPageDisplay from '@/components/View/Display/Display.vue'
import ViewPageMetadata from '@/components/View/Metadata/ViewPageMetadata.vue'
import NavigationOverlays from '@/components/View/Display/NavigationOverlays.vue'
import DatabaseMenu from '@Menu/SingleMenu.vue'
import { useShareStore } from '@/store/shareStore'
import { leaveView } from '@utils/leaveView'

const dataStore = useDataStore('mainId')
const configStore = useConfigStore('mainId')
const shareStore = useShareStore('mainId')
const route = useRoute()
const router = useRouter()

// The info sidepane is opt-in per view session: it starts closed and resets
// when the view is re-entered, rather than persisting like a global setting.
const showMetadataPanel = ref(false)

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

const share = computed(() => shareStore.resolvedShare?.share ?? null)

const nextHash = computed(() => {
  const nextIndex = index.value
  if (nextIndex === undefined) return undefined
  const nextData = dataStore.data.get(nextIndex + 1)
  if (nextData?.type === 'image' || nextData?.type === 'video') return nextData.id
  return undefined
})

const previousHash = computed(() => {
  const prevIndex = index.value
  if (prevIndex === undefined) return undefined
  const previousData = dataStore.data.get(prevIndex - 1)
  if (previousData?.type === 'image' || previousData?.type === 'video') return previousData.id
  return undefined
})

const nextPage = computed(() => {
  if (nextHash.value === undefined) return undefined
  if (route.meta.level === 2) {
    const updatedParams = { ...route.params, hash: nextHash.value }
    return { ...route, params: updatedParams }
  }
  return undefined
})

const previousPage = computed(() => {
  if (previousHash.value === undefined) return undefined
  if (route.meta.level === 2) {
    const updatedParams = { ...route.params, hash: previousHash.value }
    return { ...route, params: updatedParams }
  }
  return undefined
})

function toggleMetadataPanel() {
  showMetadataPanel.value = !showMetadataPanel.value
}

function handleClose() {
  leaveView(route, router)
}

const handleKeyDown = (event: KeyboardEvent) => {
  if (event.key !== 'Escape') return
  leaveView(route, router)
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
}

.view-stage {
  position: relative;
  height: 100%;
  width: 100%;
}

.view-stage--desktop {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  background: black;
}

.view-container {
  display: flex;
  flex-direction: row;
  width: 100vw;
  height: 100vh;
}

.view-modal {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  background: black;
  overflow: hidden;
  display: flex;
  align-items: center;
  justify-content: center;
}

.view-image-container {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.view-modal-controls {
  position: absolute;
  top: 8px;
  right: 8px;
  z-index: 1002;
  display: flex;
  gap: 4px;
}

.view-modal-sidepane {
  flex: 0 0 360px;
  height: 100vh;
  z-index: 1001;
  background: var(--v-theme-surface);
}

@media (width <= 720px) {
  .view-modal-sidepane {
    width: 100%;
    height: auto;
  }
}
</style>
