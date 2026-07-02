<template>
  <div class="pa-0 h-100 w-100 position-relative bg-background">
    <template v-if="index !== undefined">
      <div
        class="view-stage"
        :class="{ 'view-stage--desktop': !configStore.isMobile, 'panel-open': panelOpen }"
        @click.self="handleClose"
      >
        <div class="view-modal" :style="!configStore.isMobile ? modalStyle : undefined">
          <ViewPageDisplay
            :abstract-data="abstractData"
            :index="index"
            :hash="hash"
            isolation-id="mainId"
          />
          <div class="view-modal-controls">
            <v-btn icon size="small" variant="text" aria-label="Info" @click="toggleMetadataPanel">
              <v-icon>{{
                showMetadataPanel ? 'mdi-information' : 'mdi-information-outline'
              }}</v-icon>
            </v-btn>
            <v-btn icon size="small" variant="text" aria-label="Close" @click="handleClose">
              <v-icon>mdi-close</v-icon>
            </v-btn>
          </div>
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
import { leaveView } from '@utils/leaveView'

const dataStore = useDataStore('mainId')
const configStore = useConfigStore('mainId')
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

const panelOpen = computed(() => showMetadataPanel.value && !configStore.isMobile)

// Shrink-wrap the modal to the media's own aspect ratio (capped to the
// viewport) instead of a fixed max box, so it reads as "the size of the
// image" rather than a big empty frame around small images.
const modalStyle = computed(() => {
  const data = abstractData.value
  if (data && (data.type === 'image' || data.type === 'video')) {
    const ratio = data.width / data.height
    if (Number.isFinite(ratio) && ratio > 0) {
      return {
        width: `min(92vw, 92vh * ${ratio})`,
        height: `min(92vh, 92vw / ${ratio})`
      }
    }
  }
  return { width: '92vw', height: '92vh' }
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

.view-stage--desktop.panel-open {
  right: 360px;
}

.view-modal {
  position: relative;
  height: 100%;
  width: 100%;
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
  position: fixed;
  top: 0;
  right: 0;
  height: 100%;
  z-index: 1001;
}

@media (width <= 720px) {
  .view-modal-sidepane {
    width: 100%;
  }
}
</style>
