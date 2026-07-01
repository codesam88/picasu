<template>
  <v-list-item
    prepend-icon="mdi-information-outline"
    value="album-info"
    @click="modalStore.showAlbumInfoModal = true"
  >
    <v-list-item-title class="wrap">Album Info</v-list-item-title>
  </v-list-item>
</template>

<script lang="ts" setup>
import { useModalStore } from '@/store/modalStore'

const modalStore = useModalStore('mainId')
</script>
