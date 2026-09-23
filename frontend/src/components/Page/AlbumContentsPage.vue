<template>
  <PageTemplate>
    <template #content>
      <GalleryMain :key="albumId" :basic-string="basicString" />
    </template>
  </PageTemplate>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import GalleryMain from '@/components/Gallery/GalleryMain.vue'
import PageTemplate from './PageLayout/PageTemplate.vue'

const route = useRoute()

const albumId = computed(() => {
  const id = route.params.albumId
  return typeof id === 'string' ? id : ''
})

const basicString = computed(() => {
  if (!albumId.value) return null
  return `and(trashed:false, or(album:"${albumId.value}", parent_album:"${albumId.value}"))`
})
</script>
