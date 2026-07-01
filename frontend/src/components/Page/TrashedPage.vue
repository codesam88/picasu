<template>
  <PageTemplate>
    <template #content>
      <GalleryMain :basic-string="basicString" />
    </template>
  </PageTemplate>
</template>

<script setup lang="ts">
import GalleryMain from '@/components/Gallery/GalleryMain.vue'
import PageTemplate from './PageLayout/PageTemplate.vue'
const basicString = 'trashed:true'
</script>
