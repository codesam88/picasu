<template>
  <v-list-item>
    <template #prepend>
      <v-avatar>
        <v-icon>mdi-folder</v-icon>
      </v-avatar>
    </template>
    <v-list-item-subtitle class="text-wrap">
      <v-chip
        v-if="props.album"
        variant="flat"
        color="primary"
        link
        class="ma-1"
        @click="navigateToAlbum(props.album, router)"
      >
        {{ albumStore.albums.get(props.album)?.displayName ?? props.album }}
      </v-chip>
      <span v-else class="text-medium-emphasis text-caption ml-1">No album</span>
    </v-list-item-subtitle>
    <v-list-item-subtitle>
      <v-chip
        prepend-icon="mdi-pencil"
        color="surface-variant"
        variant="outlined"
        class="ma-1"
        link
        @click="openAssignAlbumModal"
        >assign</v-chip
      >
    </v-list-item-subtitle>
  </v-list-item>
</template>

<script setup lang="ts">
import { useRouter } from 'vue-router'
import { useModalStore } from '@/store/modalStore'
import { useAlbumStore } from '@/store/albumStore'
import { IsolationId } from '@type/types'
import { navigateToAlbum } from '@/route/navigator'

const props = defineProps<{
  isolationId: IsolationId
  index: number
  album: string | null
}>()

const modalStore = useModalStore('mainId')
const albumStore = useAlbumStore('mainId')
const router = useRouter()

function openAssignAlbumModal() {
  modalStore.assignAlbumBatch = false
  modalStore.showAssignAlbumModal = true
}
</script>
