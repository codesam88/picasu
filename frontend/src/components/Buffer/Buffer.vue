<template>
  <div
    id="buffer"
    class="position-relative w-100 overflow-y-hidden"
    :style="{ height: `${totalHeight}px` }"
  >
    <div
      v-for="chunk in loadedChunks"
      :key="chunk.chunkIndex"
      class="chunk-wrapper"
      :style="{
        transform: `translateY(${chunk.startY}px)`,
        position: 'absolute',
        width: '100%'
      }"
    >
      <ImageChunk
        :images="chunk.images"
        :start-index="chunk.chunkIndex * chunkSize"
        :isolation-id="isolationId"
        :timestamp="timestamp"
      />
    </div>
    <div ref="sentinel" class="sentinel"></div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount, Ref } from 'vue'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useChunkStore } from '@/store/chunkStore'
import { IsolationId, DisplayElement } from '@type/types'
import ImageChunk from '@/components/Buffer/ImageChunk.vue'
import { fetchRowInWorker } from '@/api/fetchRow'
import { getInjectValue } from '@utils/getter'

const props = defineProps<{
  isolationId: IsolationId
  bufferHeight: number
}>()

const prefetchStore = usePrefetchStore(props.isolationId)
const chunkStore = useChunkStore(props.isolationId)

const windowWidth = getInjectValue<Ref<number>>('windowWidth')
const sentinel = ref<HTMLElement | null>(null)
const timestamp = ref(Date.now())
const chunkSize = 50

const totalHeight = computed(() => {
  return Math.ceil(prefetchStore.dataLength / chunkSize) * 400
})

const loadedChunks = computed(() => {
  const result: { chunkIndex: number; images: DisplayElement[]; startY: number }[] = []
  chunkStore.loadedChunks.forEach((chunkIndex) => {
    const images = chunkStore.chunks.get(chunkIndex)
    if (images) {
      result.push({
        chunkIndex,
        images,
        startY: chunkIndex * 400
      })
    }
  })
  return result
})

let observer: IntersectionObserver | null = null

onMounted(() => {
  observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          void loadNextChunk()
        }
      })
    },
    { rootMargin: '200px' }
  )

  if (sentinel.value) {
    observer.observe(sentinel.value)
  }
})

onBeforeUnmount(() => {
  if (observer) {
    observer.disconnect()
  }
})

const loadNextChunk = async () => {
  const nextChunkIndex = chunkStore.currentChunkIndex
  const startIndex = nextChunkIndex * chunkSize

  if (startIndex >= prefetchStore.dataLength) return

  await fetchRowInWorker(nextChunkIndex, props.isolationId)
  chunkStore.currentChunkIndex++
  timestamp.value = Date.now()
}

watch(windowWidth, () => {
  chunkStore.clearAll()
  timestamp.value = Date.now()
})
</script>

<style scoped>
#buffer {
  -ms-overflow-style: none;
  scrollbar-width: none;
}

#buffer::-webkit-scrollbar {
  display: none;
}

.sentinel {
  height: 1px;
  width: 100%;
}
</style>
