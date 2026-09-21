<template>
  <v-list-item
    exact
    prepend-icon="mdi-calendar-search-outline"
    :to="`/timeline?locate=${props.assetId}`"
  >
    <v-list-item-title class="wrap">{{ 'Find In Timeline' }}</v-list-item-title>
  </v-list-item>
</template>
<script setup lang="ts">
const props = defineProps<{
  assetId: string
}>()
</script>
