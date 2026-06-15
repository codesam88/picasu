<template>
  <v-list-item prepend-icon="mdi-folder-move" @click="openModal">
    <v-list-item-title class="wrap">Assign Album</v-list-item-title>
  </v-list-item>
</template>

<script lang="ts" setup>
import { useModalStore } from '@/store/modalStore'

const modalStore = useModalStore('mainId')

function openModal() {
  modalStore.assignAlbumBatch = true
  modalStore.showAssignAlbumModal = true
}
</script>
