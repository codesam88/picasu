<template>
  <v-tooltip location="top" text="Close">
    <template #activator="{ props }">
      <v-btn v-bind="props" icon="mdi-close" @click="collectionStore.leaveEdit" />
    </template>
  </v-tooltip>
</template>

<script lang="ts" setup>
import { useCollectionStore } from '@/store/collectionStore'
import { useRoute } from 'vue-router'
import { getIsolationIdByRoute } from '@utils/getter'
const route = useRoute()
const isolationId = getIsolationIdByRoute(route)

const collectionStore = useCollectionStore(isolationId)
</script>
