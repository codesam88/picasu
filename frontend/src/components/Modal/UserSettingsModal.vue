<template>
  <BaseModal v-model="show" title="Settings" width="600">
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

const modalStore = useModalStore('mainId')

const show = computed({
  get: () => modalStore.showUserSettingsModal,
  set: (val) => {
    modalStore.showUserSettingsModal = val
  }
})
</script>
