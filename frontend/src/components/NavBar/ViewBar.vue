<template>
  <v-toolbar
    :class="[
      { 'position-absolute': constStore.viewBarOverlay },
      { 'my-toolbar': constStore.viewBarOverlay },
      { 'push-mode': !constStore.viewBarOverlay },
      { 'bg-surface-light': !constStore.viewBarOverlay }
    ]"
    :style="{
      paddingTop: '2px'
    }"
  >
    <v-spacer></v-spacer>
    <template v-if="route.meta.baseName !== 'share'">
      <v-btn
        v-if="abstractData && (abstractData.type === 'image' || abstractData.type === 'video')"
        :icon="abstractData.isFavorite ? 'mdi-star' : 'mdi-star-outline'"
        @click="setFavorite([index], !abstractData.isFavorite, isolationId)"
      ></v-btn>
    </template>
  </v-toolbar>
</template>
<script setup lang="ts">
import { setFavorite } from '@/api/editFlags'
import { EnrichedUnifiedData, IsolationId } from '@type/types'
import { useRoute } from 'vue-router'
import { onMounted } from 'vue'
import { useConstStore } from '@/store/constStore'

const route = useRoute()

const props = defineProps<{
  isolationId: IsolationId
  hash: string
  index: number
  abstractData: EnrichedUnifiedData | undefined
}>()

// Use props.isolationId for constStore to support multi-window isolation if needed,
// though constStore is usually global 'mainId' in other files, but here we can stick to props or mainId.
// The TODO example used props.isolationId.
const constStore = useConstStore(props.isolationId)

onMounted(() => {
  constStore.loadViewBarOverlay().catch((error: unknown) => {
    console.error('Failed to load viewBarOverlay:', error)
  })
})
</script>
<style scoped>
.my-toolbar {
  z-index: 2;
  background: linear-gradient(
    to bottom,
    rgba(0, 0, 0, 0.5) 0%,
    rgba(0, 0, 0, 0.25) 50%,
    rgba(0, 0, 0, 0) 100%
  );
}
</style>
