<template>
  <div class="h-100 w-100">
    <ViewPageDisplayDatabase
      v-if="
        abstractData &&
        (abstractData.type === 'image' || abstractData.type === 'video') &&
        !constStore.disableImg
      "
      :index="index"
      :hash="abstractData.id"
      :abstract-data="abstractData"
      :isolation-id="isolationId"
      :enable-watch="true"
    />
  </div>
</template>

<script setup lang="ts">
import { useConstStore } from '@/store/constStore'
import ViewPageDisplayDatabase from './DisplayDatabase.vue'
import type { EnrichedUnifiedData, IsolationId } from '@type/types'

defineProps<{
  isolationId: IsolationId
  assetId: string
  index: number
  abstractData: EnrichedUnifiedData | undefined
}>()

const constStore = useConstStore('mainId')
</script>
