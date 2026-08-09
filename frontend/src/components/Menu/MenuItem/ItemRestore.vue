<template>
  <v-list-item prepend-icon="mdi-restore" value="restore" @click="openRestoreModal">
    <v-list-item-title class="wrap">Restore</v-list-item-title>
  </v-list-item>
</template>
<script lang="ts" setup>
import { useModalStore } from '@/store/modalStore'

const props = defineProps<{
  indexList: number[]
}>()

const modalStore = useModalStore('mainId')

function openRestoreModal() {
  // Restore always opens the album-selection dialog. A single item restores
  // to its original album by default; a multi-select restores all items to the
  // one chosen target. `restoreIndexList` carries the explicit target indices
  // so the modal does not depend on edit-mode or route state.
  modalStore.assignAlbumBatch = props.indexList.length > 1
  modalStore.assignAlbumRestore = true
  modalStore.restoreIndexList = props.indexList
  modalStore.showAssignAlbumModal = true
}
</script>
