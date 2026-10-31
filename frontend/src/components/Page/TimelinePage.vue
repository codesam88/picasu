<template>
  <PageTemplate preset="full">
    <template #content>
      <div class="d-flex flex-column h-100">
        <div class="px-2 pt-1">
          <v-switch
            v-model="includePersonal"
            density="compact"
            hide-details
            label="Include personal items in timeline"
            @change="onToggle"
          />
        </div>
        <GalleryMain class="flex-grow-1 min-h-0" :basic-string="basicString" />
      </div>
    </template>
  </PageTemplate>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useRoute } from 'vue-router'
import GalleryMain from '@/components/Gallery/GalleryMain.vue'
import PageTemplate from './PageLayout/PageTemplate.vue'
import { getIncludePersonal, setIncludePersonal } from '@/script/utils/personalTimelineToggle'
import { refreshGalleryAfterMutation } from '@/script/hook/usePrefetch'

const route = useRoute()
const basicString = 'and(not(type:"album"), trashed:false)'

const includePersonal = ref(getIncludePersonal())

async function onToggle(value: boolean | string | null) {
  setIncludePersonal(value === true || value === 'true')
  await refreshGalleryAfterMutation('mainId', route)
}
</script>
