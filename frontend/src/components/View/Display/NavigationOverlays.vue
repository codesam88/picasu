<template>
  <div>
    <v-btn
      v-if="show && previousHash !== undefined"
      variant="plain"
      class="nav-btn nav-left"
      :to="previousPage"
      replace
      aria-label="Previous"
    >
      <v-icon>mdi-arrow-left</v-icon>
    </v-btn>
    <v-btn
      v-if="show && nextHash !== undefined"
      variant="plain"
      class="nav-btn nav-right"
      :to="nextPage"
      replace
      aria-label="Next"
    >
      <v-icon>mdi-arrow-right</v-icon>
    </v-btn>
  </div>
</template>

<script setup lang="ts">
import type { RouteLocationRaw } from 'vue-router'
defineProps<{
  previousHash: string | undefined
  nextHash: string | undefined
  previousPage: RouteLocationRaw | undefined
  nextPage: RouteLocationRaw | undefined
  show: boolean
}>()
</script>

<style scoped>
.nav-btn {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 1;
  inline-size: 40px;
  block-size: 60%;
  box-shadow: none;
  border-radius: 4px;
  background: rgba(var(--v-theme-on-surface), 0.15);
  color: rgb(var(--v-theme-on-surface));
}
.nav-btn:hover {
  background: rgba(var(--v-theme-on-surface), 0.3);
}
.nav-left {
  left: 0;
}
.nav-right {
  right: 0;
}
</style>
