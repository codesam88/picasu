<template>
  <div class="h-100 py-2" :style="{ width: `${scrollBarWidth}px` }">
    <div
      v-if="imageContainerRef && totalImages > 0"
      class="h-100 v-sheet"
      ref="scrollbarRef"
      id="scroll-bar"
      :style="{
        position: 'relative',
        zIndex: 3,
        cursor: `vertical-text`,
        touchAction: 'none',
        overscrollBehavior: 'contain'
      }"
      @click="handleClick"
      @mousedown="handleMouseDown"
      @mouseup="handleMouseUp"
      @mousemove="handleHover"
      @mouseleave="handleMouseLeave"
      @touchstart="handleTouchStart"
      @touchend="handleTouchEnd"
      @touchmove="handleMove"
    >
      <v-sheet
        id="main-sheet"
        class="position-relative w-100 h-100"
        :style="{ pointerEvents: 'none' }"
      >
        <v-sheet
          v-if="scrollbarRef"
          class="w-100 position-absolute bg-transparent"
          :style="{
            height: `${scrollbarHeight / totalImages}px`,
            top: `${(currentImageIndex / totalImages) * 100}%`,
            borderBottom: '1px solid rgb(var(--v-theme-primary))'
          }"
        ></v-sheet>

        <v-chip
          v-for="scrollbarData in displayScrollbarDataArrayYear"
          :key="scrollbarData.index"
          size="x-small"
          variant="text"
          class="w-100 position-absolute pa-0 ma-0 d-flex align-center justify-center"
          :style="{
            top: `${(scrollbarData.index / totalImages) * 100}%`,
            userSelect: 'none',
            zIndex: 3
          }"
        >
          {{ scrollbarData.year }}
        </v-chip>

        <v-sheet
          v-if="scrollbarRef && hoverLabelImageIndex !== undefined"
          id="current-block-sheet"
          :class="[
            'w-100 position-absolute',
            scrollbarStore.isHovering || scrollbarStore.isDragging
              ? 'bg-surface-light'
              : 'bg-surface'
          ]"
          :style="{
            height: `${scrollbarHeight / totalImages}px`,
            top: `${(hoverLabelImageIndex / totalImages) * 100}%`,
            borderBottom: '1px solid rgb(var(--v-theme-primary))'
          }"
        ></v-sheet>

        <v-sheet
          v-if="
            hoverLabelDate !== undefined &&
            hoverLabelImageIndex !== undefined &&
            scrollbarRef &&
            (configStore.isMobile
              ? scrollbarStore.isDragging
              : scrollbarStore.isHovering || scrollbarStore.isDragging)
          "
          id="current-month-sheet"
          class="position-absolute d-flex align-center justify-center text-body-small bg-surface"
          :style="{
            height: `25px`,
            width: `${scrollBarWidth}px`,
            top: `${labelTop}px`,
            left: `-${scrollBarWidth + 8}px`,
            zIndex: 4,
            userSelect: 'none'
          }"
        >
          {{ hoverLabelDate }}
        </v-sheet>
      </v-sheet>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, inject, Ref, computed, watch, watchEffect, onUnmounted } from 'vue'
import { clamp } from 'lodash'
import { useElementSize } from '@vueuse/core'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useScrollbarStore } from '@/store/scrollbarStore'
import { useLocationStore } from '@/store/locationStore'
import { IsolationId, ScrollbarData } from '@type/types'
import { scrollBarWidth } from '@/type/constants'
import { useConfigStore } from '@/store/configStore'

const hoverLabelImageIndex: Ref<number | undefined> = ref(undefined)
const currentImageIndex = ref(0)
const chipSize = 25

const props = defineProps<{
  isolationId: IsolationId
}>()

const locationStore = useLocationStore(props.isolationId)
const prefetchStore = usePrefetchStore(props.isolationId)
const scrollbarStore = useScrollbarStore(props.isolationId)
const configStore = useConfigStore('mainId')

const imageContainerRef = inject<Ref<HTMLElement | null>>('imageContainerRef')
const scrollbarRef = ref<HTMLElement | null>(null)

const totalImages = computed(() => prefetchStore.dataLength)
const { height: scrollbarHeight } = useElementSize(scrollbarRef)

const reachBottom = computed(() => {
  const el = imageContainerRef?.value
  if (!el) return false
  return el.scrollTop + el.clientHeight >= el.scrollHeight - 10
})

const hoverLabelDate = computed(() => {
  const idx = hoverLabelImageIndex.value
  if (idx === undefined) return undefined
  let label: string | undefined
  for (const d of scrollbarStore.scrollbarDataArray) {
    if (idx >= d.index) {
      label = `${d.year}.${d.month}`
    } else {
      break
    }
  }
  return label
})

const labelTop = computed(() => {
  const idx = hoverLabelImageIndex.value
  if (idx === undefined) return 0
  const blockBottom =
    (idx / totalImages.value) * scrollbarHeight.value + scrollbarHeight.value / totalImages.value
  return clamp(blockBottom - chipSize, 0, scrollbarHeight.value - chipSize)
})

const displayScrollbarDataArrayYear: Ref<ScrollbarData[]> = ref([])

const getTargetImageIndex = (percentage: number) => {
  const targetIndex = Math.floor(totalImages.value * percentage)
  return clamp(targetIndex, 0, totalImages.value - 1)
}

const getRelativePosition = (event: MouseEvent | TouchEvent): number => {
  const element = scrollbarRef.value
  if (!element) return 0

  const rect = element.getBoundingClientRect()
  let clientY: number

  if ('touches' in event && event.touches.length > 0) {
    const touch = event.touches[0]
    if (!touch) return 0
    clientY = touch.clientY
  } else if ('clientY' in event) {
    clientY = event.clientY
  } else {
    return 0
  }

  const relativeY = clientY - rect.top
  return Math.max(0, Math.min(relativeY, rect.height))
}

const handleClick = (event?: MouseEvent | TouchEvent) => {
  let clickPositionRelative: number

  if (event) {
    clickPositionRelative = getRelativePosition(event)
  } else {
    return
  }

  const targetImageIndex = getTargetImageIndex(clickPositionRelative / scrollbarHeight.value)

  if (targetImageIndex === currentImageIndex.value) {
    return
  }

  locationStore.locationIndex = targetImageIndex
  currentImageIndex.value = targetImageIndex
  hoverLabelImageIndex.value = targetImageIndex

  const el = imageContainerRef?.value
  if (el) {
    const percentage = targetImageIndex / totalImages.value
    el.scrollTop = percentage * (el.scrollHeight - el.clientHeight)
  }
}

const handleGlobalMouseMove = (event: MouseEvent) => {
  if (scrollbarStore.isDragging) {
    handleClick(event)
  }
}

const handleGlobalMouseUp = () => {
  scrollbarStore.isDragging = false
  window.removeEventListener('mousemove', handleGlobalMouseMove)
  window.removeEventListener('mouseup', handleGlobalMouseUp)
}

const handleGlobalTouchMove = (event: TouchEvent) => {
  if (scrollbarStore.isDragging) {
    handleClick(event)
  }
}

const handleGlobalTouchEnd = () => {
  scrollbarStore.isDragging = false
  window.removeEventListener('touchmove', handleGlobalTouchMove)
  window.removeEventListener('touchend', handleGlobalTouchEnd)
}

const handleMove = (event?: MouseEvent | TouchEvent) => {
  if (scrollbarStore.isDragging && event) {
    handleClick(event)
  }
}

const handleHover = (event?: MouseEvent) => {
  if (event) {
    const hoverPositionRelative = getRelativePosition(event)
    const targetImageIndex = getTargetImageIndex(hoverPositionRelative / scrollbarHeight.value)

    if (targetImageIndex >= 0 && targetImageIndex <= totalImages.value - 1) {
      hoverLabelImageIndex.value = targetImageIndex
    }
  }
  scrollbarStore.isHovering = true

  if (scrollbarStore.isDragging) {
    handleMove(event)
  }
}

const handleMouseDown = (event: MouseEvent) => {
  scrollbarStore.isDragging = true
  handleClick(event)
  window.addEventListener('mousemove', handleGlobalMouseMove)
  window.addEventListener('mouseup', handleGlobalMouseUp)
}

const handleMouseUp = () => {
  scrollbarStore.isDragging = false
  window.removeEventListener('mousemove', handleGlobalMouseMove)
  window.removeEventListener('mouseup', handleGlobalMouseUp)
}

const handleMouseLeave = () => {
  hoverLabelImageIndex.value = undefined
  scrollbarStore.isHovering = false
}

const handleTouchStart = (event: TouchEvent) => {
  scrollbarStore.isDragging = true
  handleClick(event)
  window.addEventListener('touchmove', handleGlobalTouchMove)
  window.addEventListener('touchend', handleGlobalTouchEnd)
}

const handleTouchEnd = () => {
  scrollbarStore.isDragging = false
  window.removeEventListener('touchmove', handleGlobalTouchMove)
  window.removeEventListener('touchend', handleGlobalTouchEnd)
}

watchEffect(() => {
  const array: ScrollbarData[] = []
  let lastIndex: number | null = null

  scrollbarStore.scrollbarDataArrayYear.forEach((scrollbarData) => {
    if (lastIndex === null || scrollbarData.index - lastIndex >= 1000) {
      lastIndex = scrollbarData.index
      array.push(scrollbarData)
    }
  })
  displayScrollbarDataArrayYear.value = array
})

watch([() => locationStore.locationIndex, reachBottom], () => {
  hoverLabelImageIndex.value = locationStore.locationIndex
  if (reachBottom.value) {
    currentImageIndex.value = totalImages.value - 1
  } else {
    currentImageIndex.value = locationStore.locationIndex
  }
})

onUnmounted(() => {
  window.removeEventListener('mousemove', handleGlobalMouseMove)
  window.removeEventListener('mouseup', handleGlobalMouseUp)
  window.removeEventListener('touchmove', handleGlobalTouchMove)
  window.removeEventListener('touchend', handleGlobalTouchEnd)
})
</script>
