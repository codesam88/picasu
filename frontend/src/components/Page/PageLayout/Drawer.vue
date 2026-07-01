<template>
  <v-navigation-drawer v-model="showDrawer" temporary touchless width="150" class="no-select">
    <v-list nav :key="route.fullPath" :disabled="!initializedStore.initialized">
      <v-list-item slim to="/" prepend-icon="mdi-home" title="Home"></v-list-item>
      <v-list-item slim to="/albums" prepend-icon="mdi-image-album" title="Albums"></v-list-item>
      <v-list-item slim to="/tags" prepend-icon="mdi-tag-multiple" title="Tags"></v-list-item>
      <v-divider></v-divider>
    </v-list>

    <template #append>
      <v-list nav :key="route.fullPath" :disabled="!initializedStore.initialized">
        <v-list-item slim to="/trashed" prepend-icon="mdi-trash-can" title="Trashed"></v-list-item>
        <v-list-item slim to="/config" prepend-icon="mdi-tune" title="Config"></v-list-item>
      </v-list>
    </template>
  </v-navigation-drawer>
</template>

<script setup lang="ts">
import { inject, type Ref } from 'vue'
import { useRoute } from 'vue-router'
import { useInitializedStore } from '@/store/initializedStore'

const showDrawer = inject<Ref<boolean>>('showDrawer')
const route = useRoute()
const initializedStore = useInitializedStore('mainId')
</script>

<style scoped></style>
