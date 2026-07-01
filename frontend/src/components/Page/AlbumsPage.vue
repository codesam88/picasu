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
const basicString = 'and(type:"album", trashed:false, root_album:true)'
</script>
