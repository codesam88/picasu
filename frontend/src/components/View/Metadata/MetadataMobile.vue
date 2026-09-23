<template>
  <div class="swiper-container h-100 w-100">
    <swiper
      :modules="modules"
      :slides-per-view="1"
      :space-between="10"
      :centered-slides="true"
      :initial-slide="currentSlideIndex"
      :resistance="true"
      :resistance-ratio="0.3"
      :allow-touch-move="true"
      @slide-change="onSlideChange"
      @swiper="onSwiper"
      class="h-100"
    >
      <swiper-slide v-if="previousAssetId !== undefined">
        <div class="slide-content">
          <MetadataContent
            v-if="previousAbstractData"
            :abstract-data="previousAbstractData"
            :index="index - 1"
            :asset-id="previousAssetId"
            :isolation-id="isolationId"
            compact
          />
        </div>
      </swiper-slide>

      <swiper-slide>
        <div class="slide-content">
          <MetadataContent
            v-if="abstractData"
            :abstract-data="abstractData"
            :index="index"
            :asset-id="assetId"
            :isolation-id="isolationId"
            compact
          />
        </div>
      </swiper-slide>

      <swiper-slide v-if="nextAssetId !== undefined">
        <div class="slide-content">
          <MetadataContent
            v-if="nextAbstractData"
            :abstract-data="nextAbstractData"
            :index="index + 1"
            :asset-id="nextAssetId"
            :isolation-id="isolationId"
            compact
          />
        </div>
      </swiper-slide>
    </swiper>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Swiper, SwiperSlide } from 'swiper/vue'
import { Manipulation } from 'swiper/modules'
import type { Swiper as SwiperType } from 'swiper'
import { useDataStore } from '@/store/dataStore'
import type { EnrichedUnifiedData, IsolationId } from '@type/types'
import MetadataContent from './MetadataContent.vue'
import 'swiper/css'
import 'swiper/css/manipulation'

const props = defineProps<{
  isolationId: IsolationId
  assetId: string
  index: number
  abstractData: EnrichedUnifiedData
}>()

const dataStore = useDataStore(props.isolationId)
const route = useRoute()
const router = useRouter()

const modules = [Manipulation]
const swiperInstance = ref<SwiperType | null>(null)

const nextAbstractData = computed(() => dataStore.data.get(props.index + 1))
const previousAbstractData = computed(() => dataStore.data.get(props.index - 1))

const nextAssetId = computed(() => {
  const nextData = nextAbstractData.value
  if (nextData?.type === 'image' || nextData?.type === 'video') return nextData.assetId
  if (nextData?.type === 'album') return nextData.id
  return undefined
})

const previousAssetId = computed(() => {
  const prevData = previousAbstractData.value
  if (prevData?.type === 'image' || prevData?.type === 'video') return prevData.assetId
  if (prevData?.type === 'album') return prevData.id
  return undefined
})

const currentSlideIndex = computed(() => (previousAssetId.value !== undefined ? 1 : 0))

function onSwiper(swiper: SwiperType) {
  swiperInstance.value = swiper
}

function onSlideChange(swiper: SwiperType) {
  const currentIndex = swiper.activeIndex
  const hasPrevious = previousAssetId.value !== undefined
  const hasNext = nextAssetId.value !== undefined

  if (hasPrevious) {
    if (currentIndex === 0 && previousAssetId.value) {
      navigateToAsset(previousAssetId.value)
    } else if (currentIndex === 2 && hasNext && nextAssetId.value) {
      navigateToAsset(nextAssetId.value)
    }
  } else if (currentIndex === 1 && hasNext && nextAssetId.value) {
    navigateToAsset(nextAssetId.value)
  }
}

function navigateToAsset(targetAssetId: string) {
  if (route.meta.level === 2) {
    const updatedParams = { ...route.params, assetId: targetAssetId }
    void router.replace({
      name: route.name ?? undefined,
      params: updatedParams,
      query: route.query
    })
  }
}

watch(
  () => props.index,
  () => {
    if (swiperInstance.value) {
      swiperInstance.value.slideTo(currentSlideIndex.value, 0)
    }
  }
)
</script>

<style scoped>
.swiper-container {
  width: 100%;
  height: 100%;
  overflow: hidden;
  touch-action: pan-y;
}

.slide-content {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

:deep(.swiper) {
  width: 100%;
  height: 100%;
  overflow: hidden;
}

:deep(.swiper-slide) {
  background: transparent;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}
</style>
