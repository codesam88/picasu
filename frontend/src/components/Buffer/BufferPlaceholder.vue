<template>
  <div
    ref="placeholderRef"
    class="w-100 position-absolute"
    :style="{
      transform: `translateY(${modifyTopPixel ? topPixel - placeholderRefHeight : topPixel}px)`,
      willChange: 'transform'
    }"
  >
    <div v-for="index in placeholderRowNumScaled" :key="`extra-${index}`" class="d-flex flex-wrap">
      <div
        v-for="subindex in placeholderColNum"
        ref="placeholderRowRef"
        :key="`extra-${subindex}`"
        class="bg-placeholder ma-1"
        :style="{
          position: 'relative',
          width: `${placeholderWidth}px`
        }"
      >
        <i class="d-block" :style="{ paddingBottom: `${placeHolderHeight}px` }"></i>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useConstStore } from '@/store/constStore'
import { paddingPixel } from '@/type/constants'
import { getInjectValue } from '@utils/getter'
import { rowBaseHeight } from '@utils/rowHeight'
import { computed, onMounted, Ref, ref, watchEffect } from 'vue'
const constStore = useConstStore('mainId')

const placeholderRef = ref<HTMLElement>()
const windowWidth = getInjectValue<Ref<number>>('windowWidth')
const windowHeight = getInjectValue<Ref<number>>('windowHeight')

const placeholderRefHeight = ref(0)
const placeholderRowRef = ref<HTMLElement[]>([])
const placeholderRowRefHeight = ref(0)

defineProps<{
  topPixel: number
  modifyTopPixel: boolean
}>()

// Same call and inputs as the layout worker: rows are exactly this tall
const placeHolderHeight = computed(() => {
  return rowBaseHeight(windowWidth.value, constStore.subRowHeightScale)
})

// 3:2 cells echo the loaded geometry: width follows the shared row height
const placeholderWidth = computed(() => {
  return 1.5 * placeHolderHeight.value
})

const placeholderWidthWithPadding = computed(() => {
  return placeholderWidth.value + 2 * paddingPixel
})

const placeholderColNum = computed(() => {
  return Math.floor(windowWidth.value / placeholderWidthWithPadding.value)
})

const placeholderRowNum = computed(() => {
  return placeHolderHeight.value > 0 ? Math.ceil(windowHeight.value / placeHolderHeight.value) : 0
})

const placeholderRowNumScaled = computed(() => {
  return Math.ceil(2 * placeholderRowNum.value)
})

onMounted(() => {
  watchEffect(() => {
    if (placeholderRef.value && placeholderRef.value.clientHeight > 0) {
      placeholderRefHeight.value = placeholderRef.value.clientHeight
    }
  })
  watchEffect(() => {
    if (placeholderRowRef.value[0] !== undefined && placeholderRowRef.value[0].clientHeight > 0) {
      placeholderRowRefHeight.value = placeholderRowRef.value[0].clientHeight
    }
  })
})

defineExpose({
  placeholderRowRefHeight
})
</script>
