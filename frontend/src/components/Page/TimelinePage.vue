<template>
  <PageTemplate preset="full">
    <template #content>
      <GalleryMain :basic-string="basicString" />
    </template>
  </PageTemplate>
</template>

<script setup lang="ts">
import GalleryMain from '@/components/Gallery/GalleryMain.vue'
import PageTemplate from './PageLayout/PageTemplate.vue'

const basicString = 'and(not(type:"album"), archived:false, trashed:false)'
</script>
