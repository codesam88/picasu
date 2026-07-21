<template>
  <div class="image-chunk">
    <div
      v-for="(image, localIndex) in images"
      :key="`${startIndex + localIndex}-${timestamp}`"
      class="image-item"
      :style="{
        breakInside: 'avoid',
        marginBottom: `${paddingPixel}px`
      }"
    >
      <div class="image-wrapper position-relative">
        <div
          class="click-handler"
          :class="{
            'locate-highlight': locationStore.highlightedIndex === startIndex + localIndex
          }"
          :style="{
            pointerEvents: 'none',
            zIndex: 100,
            border:
              collectionStore.editModeOn &&
              collectionStore.editModeCollection.has(startIndex + localIndex)
                ? '4px solid rgb(var(--v-theme-primary))'
                : '4px solid transparent'
          }"
          @click="(event: MouseEvent) => handleClick(event, startIndex + localIndex)"
        ></div>
        <DesktopHoverIcon
          class="icon-hover"
          v-if="!mobile"
          :on-click="(event: MouseEvent) => handleClickIcon(event, startIndex + localIndex)"
        />
        <HoverGradientDiv :mobile="mobile" />
        <MainBlock
          :index="startIndex + localIndex"
          :display-element="image"
          :isolation-id="isolationId"
          :mobile="mobile"
          :on-pointerdown="
            (event: PointerEvent) => handlePointerdown(event, startIndex + localIndex)
          "
          :on-pointerup="(event: PointerEvent) => handlePointerUp(event, startIndex + localIndex)"
          :on-pointerleave="handlePointerLeave"
          :on-click="(event: MouseEvent) => handleClick(event, startIndex + localIndex)"
        />
        <div
          class="grey-background-placeholder"
          @click="(event: MouseEvent) => handleClick(event, startIndex + localIndex)"
        ></div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onBeforeUnmount } from 'vue'
import { IsolationId, DisplayElement } from '@type/types'
import { useCollectionStore } from '@/store/collectionStore'
import { useHandleClick } from '@/script/hook/useHandleClick'
import { useRouter, useRoute } from 'vue-router'
import { useQueueStore } from '@/store/queueStore'
import { useWorkerStore } from '@/store/workerStore'
import { useScrollTopStore } from '@/store/scrollTopStore'
import { useConfigStore } from '@/store/configStore'
import { useConstStore } from '@/store/constStore'
import { useLocationStore } from '@/store/locationStore'
import { paddingPixel } from '@/type/constants'
import MainBlock from './FunctionalComponent/MainBlock'
import DesktopHoverIcon from './FunctionalComponent/DesktopHoverIcon'
import HoverGradientDiv from './FunctionalComponent/HoverGradientDiv'

const props = defineProps<{
  images: DisplayElement[]
  startIndex: number
  isolationId: IsolationId
  timestamp: number
}>()

const router = useRouter()
const route = useRoute()
const constStore = useConstStore('mainId')
const configStore = useConfigStore('mainId')
const collectionStore = useCollectionStore(props.isolationId)
const queueStore = useQueueStore(props.isolationId)
const workerStore = useWorkerStore(props.isolationId)
const scrollTopStore = useScrollTopStore(props.isolationId)
const locationStore = useLocationStore(props.isolationId)

const mobile = configStore.isMobile
const isLongPress = ref(false)
const pressTimer = ref<number | null>(null)
const scrollingTimer = ref<number | null>(null)
const isScrolling = ref(false)

watch(
  () => scrollTopStore.scrollTop,
  () => {
    isScrolling.value = true
    if (scrollingTimer.value !== null) {
      clearTimeout(scrollingTimer.value)
    }
    scrollingTimer.value = window.setTimeout(() => {
      isScrolling.value = false
      scrollingTimer.value = null
    }, 100)
  }
)

const { handleClick } = useHandleClick(router, route, props.isolationId)

const handlePointerdown = (event: MouseEvent, currentIndex: number) => {
  if (isScrolling.value) return
  isLongPress.value = false
  pressTimer.value = window.setTimeout(() => {
    isLongPress.value = true
    handleLongPressClick(event, currentIndex)
  }, 600)
}

const handlePointerUp = (event: MouseEvent, currentIndex: number) => {
  if (isScrolling.value) return
  if (pressTimer.value !== null) {
    clearTimeout(pressTimer.value)
    pressTimer.value = null
  }
  if (!isLongPress.value) {
    handleClick(event, currentIndex)
  }
}

const handlePointerLeave = () => {
  if (pressTimer.value !== null) {
    clearTimeout(pressTimer.value)
    pressTimer.value = null
  }
}

const handleLongPressClick = (event: MouseEvent, currentIndex: number) => {
  if (!collectionStore.editModeOn) {
    collectionStore.editModeOn = true
    collectionStore.addApi(currentIndex)
    collectionStore.lastClick = currentIndex
  } else {
    handleClick(event, currentIndex)
  }
}

const handleClickIcon = (event: MouseEvent, currentIndex: number) => {
  if (!collectionStore.editModeOn) {
    collectionStore.editModeOn = true
    collectionStore.addApi(currentIndex)
    collectionStore.lastClick = currentIndex
  } else {
    handleClick(event, currentIndex)
  }
}

watch(
  () => locationStore.highlightedIndex,
  (val) => {
    if (val !== null && val >= props.startIndex && val < props.startIndex + props.images.length) {
      setTimeout(() => {
        locationStore.highlightedIndex = null
      }, 2000)
    }
  }
)

onBeforeUnmount(() => {
  for (let i = 0; i < props.images.length; i++) {
    const abortIndex = props.startIndex + i
    const workerIndex = abortIndex % constStore.concurrencyNumber
    if (workerStore.postToImgWorkerList !== undefined) {
      const worker = workerStore.postToImgWorkerList[workerIndex]
      if (worker) {
        worker.processAbort({ index: abortIndex })
      }
    }
    queueStore.img.delete(abortIndex)
  }
})
</script>

<style scoped>
.image-chunk {
  column-width: 200px;
  column-gap: 4px;
}

.image-item {
  break-inside: avoid;
}

.image-wrapper {
  position: relative;
  width: 100%;
}

.click-handler {
  position: absolute;
  width: 100%;
  height: 100%;
  z-index: 100;
}

.icon-hover {
  color: #fafafa;
  transition: color 0.3s;
  cursor: pointer;
}

.icon-hover:hover {
  color: white;
}

.grey-background-placeholder {
  position: absolute;
  width: 100%;
  height: 100%;
  z-index: 0;
  background: rgb(var(--v-theme-surface));
}

.locate-highlight {
  animation: locate-pulse 2s ease-out forwards;
}

@keyframes locate-pulse {
  0% {
    box-shadow: inset 0 0 0 4px rgba(255, 193, 7, 0.9);
  }
  100% {
    box-shadow: inset 0 0 0 4px transparent;
  }
}
</style>
