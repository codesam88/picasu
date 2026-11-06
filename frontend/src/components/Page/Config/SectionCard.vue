<template>
  <v-card border flat>
    <v-card-title class="font-weight-bold">{{ title }}</v-card-title>
    <v-divider thickness="4" variant="double"></v-divider>

    <v-card-text v-if="description" class="text-medium-emphasis">
      {{ description }}
    </v-card-text>

    <slot></slot>

    <v-card-actions v-if="$slots.actions" class="justify-end px-4 pb-4">
      <slot name="actions"></slot>
    </v-card-actions>
  </v-card>
</template>

<script setup lang="ts">
defineProps<{
  title: string
  description?: string
}>()
</script>
