<template>
  <BaseModal v-model="show" title="Settings" width="600">
    <template #header>
      <v-toolbar color="transparent" density="compact">
        <v-toolbar-title>{{ headerTitle }}</v-toolbar-title>

        <template #append>
          <v-chip
            v-if="currentUser != null"
            :color="currentUser?.isAdmin ? 'primary' : 'default'"
            density="compact"
            class="mr-2"
          >
            {{ currentUser?.isAdmin ? 'Admin' : 'Standard' }}
          </v-chip>
          <v-btn icon="mdi-close" variant="text" density="comfortable" @click="show = false" />
        </template>
      </v-toolbar>
    </template>
    <FrontendConfig />
    <v-divider class="my-2" />
    <ChangePassword />
  </BaseModal>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useModalStore } from '@/store/modalStore'
import BaseModal from '@/components/Modal/BaseModal.vue'
import FrontendConfig from '@/components/Page/Config/FrontendConfig.vue'
import ChangePassword from '@/components/Page/Config/ChangePassword.vue'
import { useCurrentUser } from '@/script/utils/currentUser'

const modalStore = useModalStore('mainId')

const show = computed({
  get: () => modalStore.showUserSettingsModal,
  set: (val) => {
    modalStore.showUserSettingsModal = val
  }
})

// Read-only identity decoded from the session cookie; null when signed out
// or on a share token — the header falls back to the generic title.
const currentUser = useCurrentUser()

const headerTitle = computed(() => {
  const id = currentUser.value?.id ?? ''
  return id !== '' ? `Settings — ${id}` : 'Settings'
})
</script>
