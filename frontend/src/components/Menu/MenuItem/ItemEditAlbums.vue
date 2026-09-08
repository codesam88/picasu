<template>
  <v-list-item prepend-icon="mdi-folder-move" value="assign-album" @click="openModal">
    <v-list-item-title class="wrap">{{ label }}</v-list-item-title>
  </v-list-item>
</template>
<script setup lang="ts">
import { useModalStore } from '@/store/modalStore'

withDefaults(defineProps<{ label?: string }>(), { label: 'Assign Album' })

const modalStore = useModalStore('mainId')

function openModal() {
  modalStore.assignAlbumBatch = false
  modalStore.showAssignAlbumModal = true
}
</script>
