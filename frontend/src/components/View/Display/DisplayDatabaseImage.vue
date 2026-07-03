<template>
  <img
    :key="index"
    v-if="abstractData?.type === 'image' && imgStore.imgOriginal.get(index)"
    :src="imgStore.imgOriginal.get(index)"
    :style="{
      maxWidth: '98%',
      maxHeight: '98%',
      width: 'auto',
      height: 'auto',
      objectFit: 'contain',
      border: '2px solid rgba(255, 255, 255, 0.8)',
      transform: `rotate(${-(editStore.rotationCounts.get(abstractData?.id ?? '') ?? 0) * 90}deg)`,
      transition: 'transform 0.3s ease'
    }"
  />
</template>

<script setup lang="ts">
import { useImgStore } from '@/store/imgStore'
import { useEditStore } from '@/store/editStore'
import { EnrichedUnifiedData, IsolationId } from '@type/types'

const props = defineProps<{
  isolationId: IsolationId
  index: number
  abstractData: EnrichedUnifiedData
}>()

const imgStore = useImgStore(props.isolationId)
const editStore = useEditStore('mainId')
</script>
