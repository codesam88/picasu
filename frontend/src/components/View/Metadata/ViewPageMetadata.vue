<template>
  <div
    id="abstractData-col"
    v-if="abstractData"
    class="h-100 flex-grow-0 flex-shrink-0 bg-surface"
    style="z-index: 1"
  >
    <MetadataMobile
      v-if="configStore.isMobile"
      :abstract-data="abstractData"
      :index="index"
      :asset-id="assetId"
      :isolation-id="isolationId"
    />
    <MetadataContent
      v-else
      :abstract-data="abstractData"
      :index="index"
      :asset-id="assetId"
      :isolation-id="isolationId"
    />
  </div>
</template>

<script setup lang="ts">
import { onMounted, watch } from 'vue'
import { useConfigStore } from '@/store/configStore'
import { useDataStore } from '@/store/dataStore'
import { fetchAssetMetadata } from '@/api/fetchMetadata'
import { EnrichedUnifiedData, IsolationId } from '@type/types'
import MetadataContent from './MetadataContent.vue'
import MetadataMobile from './MetadataMobile.vue'

const props = defineProps<{
  isolationId: IsolationId
  assetId: string
  index: number
  abstractData: EnrichedUnifiedData
}>()

const configStore = useConfigStore(props.isolationId)
const dataStore = useDataStore(props.isolationId)

/**
 * List rows no longer carry tags/EXIF/description/rating (Phase 14), so the
 * info panel fetches the detail record for the current asset whenever it is
 * opened or navigated to, then merges it into the row. Album rows already
 * carry full metadata and are skipped inside `mergeMetadata`.
 */
async function loadDetailMetadata() {
  if (props.abstractData.type === 'album') return
  const detail = await fetchAssetMetadata(props.assetId, props.isolationId)
  if (detail === null) return
  // Re-resolve the index: the row may have moved while the request was in flight.
  const index = dataStore.assetIdMapData.get(props.assetId)
  if (index !== undefined) {
    dataStore.mergeMetadata(index, detail)
  }
}

onMounted(() => {
  void loadDetailMetadata()
})

watch(
  () => props.assetId,
  () => {
    void loadDetailMetadata()
  }
)
</script>

<style scoped>
#abstractData-col {
  width: 360px;
}
@media (width <= 720px) {
  #abstractData-col {
    width: 100%;
  }
}
</style>
