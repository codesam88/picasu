<template>
  <v-list-item>
    <template #prepend>
      <v-avatar>
        <v-icon>mdi-tag</v-icon>
      </v-avatar>
    </template>
    <v-list-item-subtitle class="text-wrap">
      <v-chip
        variant="flat"
        color="primary"
        v-for="tag in tags"
        :key="tag"
        link
        class="ma-1"
        @click="searchByTag(tag, router)"
      >
        {{ tag }}
      </v-chip>
    </v-list-item-subtitle>
    <v-list-item-subtitle v-if="route.meta.baseName !== 'share'">
      <v-chip
        prepend-icon="mdi-pencil"
        color="surface-variant"
        variant="outlined"
        class="ma-1"
        link
        @click="openEditTagsModal"
        >edit</v-chip
      >
    </v-list-item-subtitle>
  </v-list-item>
</template>

<script setup lang="ts">
import { useRoute, useRouter } from 'vue-router'
import { useModalStore } from '@/store/modalStore'
import { searchByTag } from '@utils/getter'

defineProps<{
  tags: string[]
}>()

const modalStore = useModalStore('mainId')

const route = useRoute()
const router = useRouter()

function openEditTagsModal() {
  modalStore.showEditTagsModal = true
}
</script>
