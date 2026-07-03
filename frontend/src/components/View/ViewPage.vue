<template>
  <div class="pa-0 h-100 w-100 position-relative bg-background">
    <template v-if="index !== undefined">
      <div
        class="view-stage"
        :class="{ 'view-stage--desktop': !configStore.isMobile, 'panel-open': panelOpen }"
        @click.self="handleClose"
      >
        <div class="view-container">
          <div class="view-modal" :style="!configStore.isMobile ? modalStyle : undefined">
            <NavigationOverlays
              v-if="!configStore.isMobile"
              :previous-hash="previousHash"
              :next-hash="nextHash"
              :previous-page="previousPage"
              :next-page="nextPage"
              :show="!configStore.isMobile"
            />
            <div
              class="view-image-container"
              :style="!configStore.isMobile ? imageContainerStyle : undefined"
            >
              <ViewPageDisplay
                :abstract-data="abstractData"
                :index="index"
                :hash="hash"
                isolation-id="mainId"
              />
            </div>
            <div class="view-modal-controls">
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
              <v-btn
                icon
                size="small"
                variant="text"
                aria-label="Info"
                @click="toggleMetadataPanel"
              >
                <v-icon>{{
                  showMetadataPanel ? 'mdi-information' : 'mdi-information-outline'
                }}</v-icon>
              </v-btn>
              <v-btn icon size="small" variant="text" aria-label="Close" @click="handleClose">
                <v-icon>mdi-close</v-icon>
              </v-btn>
            </div>
          </div>
          <ViewPageMetadata
            v-if="showMetadataPanel && abstractData"
            class="view-modal-sidepane"
            :style="!configStore.isMobile ? sidepaneStyle : undefined"
            :abstract-data="abstractData"
            :index="index"
            :hash="hash"
            isolation-id="mainId"
          />
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

// Reactive window dimensions for responsive modal sizing
const windowWidth = ref(window.innerWidth)
const windowHeight = ref(window.innerHeight)

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

const SIDEPANE_WIDTH = 360
const panelOpen = computed(() => showMetadataPanel.value && !configStore.isMobile)
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

// Fixed 16:9 aspect ratio modal, sized to 80% of available screen space
const NAV_ELEMENT_SIZE = 64
const MODAL_ASPECT_RATIO = 16 / 9

const modalStyle = computed(() => {
  const maxWidth = windowWidth.value * 0.8
  const maxHeight = windowHeight.value * 0.8

  // Fit 16:9 ratio within the available space
  let width = maxWidth
  let height = width / MODAL_ASPECT_RATIO

  if (height > maxHeight) {
    height = maxHeight
    width = height * MODAL_ASPECT_RATIO
  }

  return {
    width: `${width}px`,
    height: `${height}px`
  }
})

// Image container style with padding for navigation elements
const imageContainerStyle = computed(() => ({
  padding: `${NAV_ELEMENT_SIZE}px`
}))

const sidepaneStyle = computed(() => {
  if (!configStore.isMobile && showMetadataPanel.value) {
    return { height: modalStyle.value.height, width: `${SIDEPANE_WIDTH}px` }
  }
  return undefined
})

function handleResize() {
  windowWidth.value = window.innerWidth
  windowHeight.value = window.innerHeight
}

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
  window.addEventListener('resize', handleResize)
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeyDown)
  window.removeEventListener('resize', handleResize)
})
</script>
<style scoped>
.v-container::-webkit-scrollbar {
  display: none;
  /* Hide scrollbar */
}

.view-stage {
  position: relative;
  height: 100%;
  width: 100%;
}

.view-stage--desktop {
  /* width/height:auto overrides the base rule's 100% so the fixed box's
     size comes from the inset offsets below (needed for `right` to actually
     shrink the box once the sidepane opens, instead of being ignored in
     favor of an explicit width). */
  width: auto;
  height: auto;
  position: fixed;
  inset: 0;
  /* Above the persistent parent navbar (GalleryBar), below Vuetify's own
     dialog/menu overlays (~2400) so modals like Assign Album still layer
     correctly on top of this one. */
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.35);
  transition: right 0.2s ease;
}

.view-container {
  display: flex;
  align-items: stretch;
}

.view-modal {
  position: relative;
  height: 100%;
  width: 100%;
  background: black;
  overflow: hidden;
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
  z-index: 1;
  display: flex;
  gap: 4px;
  background: rgba(0, 0, 0, 0.35);
  border-radius: 24px;
  padding: 2px;
}

.view-modal-sidepane {
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
