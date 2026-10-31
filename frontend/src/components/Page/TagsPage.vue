<template>
  <PageTemplate preset="card" width="compact" :ready="tagStore.fetched">
    <template #content>
      <div v-if="tagStore.tags.length === 0">No tags yet</div>
      <div v-else class="d-flex flex-wrap align-center justify-center ga-2 pa-4">
        <v-tooltip v-for="tagsData in tagStore.tags" :key="tagsData.tag" location="top">
          <template #activator="{ props }">
            <v-chip
              v-bind="props"
              variant="tonal"
              class="ma-1"
              link
              @click="searchByTag(tagsData.tag, router)"
              :style="{ fontSize: chipFontSize(tagsData.number) + 'rem' }"
            >
              {{ tagsData.tag }}
            </v-chip>
          </template>
          <span>{{ tagsData.number }} items</span>
        </v-tooltip>
      </div>
    </template>
  </PageTemplate>
</template>

<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount } from 'vue'
import { useRouter } from 'vue-router'
import { useTagStore } from '@/store/tagStore'
import { useInitializedStore } from '@/store/initializedStore'
import { searchByTag } from '@utils/getter'
import { tagCloudWeight, tagCloudFontSize } from '@/script/utils/tagCloud'
import PageTemplate from './PageLayout/PageTemplate.vue'

const initializedStore = useInitializedStore('mainId')
const tagStore = useTagStore('mainId')
const router = useRouter()

const minCount = computed(() =>
  tagStore.tags.length === 0 ? 0 : Math.min(...tagStore.tags.map((t) => t.number))
)
const maxCount = computed(() =>
  tagStore.tags.length === 0 ? 0 : Math.max(...tagStore.tags.map((t) => t.number))
)

function chipFontSize(count: number): number {
  return tagCloudFontSize(tagCloudWeight(count, minCount.value, maxCount.value))
}

onMounted(async () => {
  if (!tagStore.fetched) {
    await tagStore.fetchTags()
  }
  initializedStore.initialized = true
})

onBeforeUnmount(() => {
  initializedStore.initialized = false
})
</script>
