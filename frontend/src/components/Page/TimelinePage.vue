<template>
  <PageTemplate preset="full">
    <template #content>
      <div class="d-flex flex-column h-100">
        <GalleryMain class="flex-grow-1 min-h-0" :basic-string="basicString" />
      </div>
    </template>
  </PageTemplate>
</template>

<script setup lang="ts">
import GalleryMain from '@/components/Gallery/GalleryMain.vue'
import PageTemplate from './PageLayout/PageTemplate.vue'

const basicString = 'and(not(type:"album"), trashed:false)'
</script>
