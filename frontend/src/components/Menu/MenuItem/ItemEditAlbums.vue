<template>
  <v-list-item
    prepend-icon="mdi-folder-move"
    value="assign-album"
    role="menuitem"
    @click="openModal"
  >
    <v-list-item-title class="wrap">Assign Album</v-list-item-title>
  </v-list-item>
</template>
<script setup lang="ts">
import { useModalStore } from '@/store/modalStore'

const modalStore = useModalStore('mainId')

function openModal() {
  modalStore.assignAlbumBatch = false
  modalStore.showAssignAlbumModal = true
}
</script>
