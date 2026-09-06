<template>
  <v-list-item prepend-icon="mdi-folder-move" @click="openModal">
    <v-list-item-title class="wrap">{{ label }}</v-list-item-title>
  </v-list-item>
</template>

<script lang="ts" setup>
import { useModalStore } from '@/store/modalStore'

withDefaults(defineProps<{ label?: string }>(), { label: 'Assign Album' })

const modalStore = useModalStore('mainId')

function openModal() {
  modalStore.assignAlbumBatch = true
  modalStore.showAssignAlbumModal = true
}
</script>
