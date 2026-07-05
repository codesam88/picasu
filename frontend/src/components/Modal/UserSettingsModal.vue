<template>
  <BaseModal v-model="show" title="Settings" width="600">
    <FrontendConfig />
    <v-divider class="my-2" />
    <ChangePassword v-model:has-password="hasPassword" />
  </BaseModal>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useModalStore } from '@/store/modalStore'
import { useConfigStore } from '@/store/configStore'
import BaseModal from '@/components/Modal/BaseModal.vue'
import FrontendConfig from '@/components/Page/Config/FrontendConfig.vue'
import ChangePassword from '@/components/Page/Config/ChangePassword.vue'

const modalStore = useModalStore('mainId')
const configStore = useConfigStore('mainId')

const hasPassword = ref(false)

const show = computed({
  get: () => modalStore.showUserSettingsModal,
  set: (val) => {
    modalStore.showUserSettingsModal = val
  }
})

onMounted(async () => {
  if (!configStore.config) {
    await configStore.fetchConfig()
  }
  hasPassword.value = configStore.config?.hasPassword ?? false
})
</script>
