<template>
  <div class="h-100 w-100">
    <ViewPageDisplayDatabase
      v-if="
        abstractData &&
        (abstractData.type === 'image' || abstractData.type === 'video') &&
        !configStore.disableImg
      "
      :index="index"
      :hash="hash"
      :abstract-data="abstractData"
      :isolation-id="isolationId"
      :enable-watch="true"
    />
  </div>
</template>

<script setup lang="ts">
import { useConfigStore } from '@/store/configStore'
import ViewPageDisplayDatabase from './DisplayDatabase.vue'
import type { EnrichedUnifiedData, IsolationId } from '@type/types'

const props = defineProps<{
  isolationId: IsolationId
  hash: string
  index: number
  abstractData: EnrichedUnifiedData | undefined
}>()

const configStore = useConfigStore(props.isolationId)
</script>
