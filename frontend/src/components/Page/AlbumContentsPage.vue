<template>
  <PageTemplate>
    <template #content>
      <GalleryMain :key="albumHash" :basic-string="basicString" />
    </template>
  </PageTemplate>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import GalleryMain from '@/components/Gallery/GalleryMain.vue'
import PageTemplate from './PageLayout/PageTemplate.vue'

const route = useRoute()

const albumHash = computed(() => {
  const id = route.params.albumHash
  return typeof id === 'string' ? id : ''
})

const basicString = computed(() => {
  if (!albumHash.value) return null
  return `and(trashed:false, or(album:"${albumHash.value}", parent_album:"${albumHash.value}"))`
})
</script>
