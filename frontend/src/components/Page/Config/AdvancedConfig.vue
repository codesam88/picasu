<template>
  <v-col cols="12">
    <v-card border flat class="danger-zone-card">
      <v-card-title class="font-weight-bold text-error">Danger Zone</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-list-item
        title="Read Only Mode"
        subtitle="Prevent modification of data and settings"
        @click="onReadOnlyToggle(!readOnlyMode)"
      >
        <template #append>
          <v-switch
            :model-value="readOnlyMode"
            color="primary"
            hide-details
            inset
            aria-label="Read Only Mode"
            :disabled="disabling"
            @update:model-value="onReadOnlyToggle"
            @click.stop
          ></v-switch>
        </template>
      </v-list-item>

      <v-divider></v-divider>
      <v-list-item
        v-if="isAdmin"
        title="Allow Registration"
        subtitle="Permit new users to register"
      >
        <template #append>
          <v-switch
            :model-value="allowRegistration"
            color="error"
            hide-details
            inset
            aria-label="Allow Registration"
            :disabled="allowRegistrationSaving"
            @update:model-value="$emit('update:allowRegistration', $event)"
            @click.stop
          ></v-switch>
        </template>
      </v-list-item>
      <v-divider v-if="isAdmin"></v-divider>
      <v-list-item title="JWT Authentication Key" subtitle="Resetting signs everyone out">
        <template #append>
          <v-btn
            color="primary"
            variant="flat"
            :loading="resetting"
            @click.stop="resetKey"
            class="text-none"
          >
            Reset Key
          </v-btn>
        </template>
      </v-list-item>
      <v-card-actions class="justify-end px-4 pb-4">
        <v-btn color="primary" variant="flat" :loading="loading" @click="save" class="text-none">
          Save Changes
        </v-btn>
      </v-card-actions>
    </v-card>

    <v-dialog v-model="confirmOpen" max-width="480">
      <v-card>
        <v-card-title class="font-weight-bold">Disable read-only mode?</v-card-title>
        <v-card-text>
          This lifts the read-only kill-switch and re-enables writes. Enter your password to
          confirm.
          <v-text-field
            v-model="disablePassword"
            type="password"
            label="Password"
            class="mt-2"
            @keyup.enter="confirmDisable"
          ></v-text-field>
        </v-card-text>
        <v-card-actions class="justify-end">
          <v-btn variant="text" @click="cancelDisable">Cancel</v-btn>
          <v-btn color="primary" variant="flat" :loading="disabling" @click="confirmDisable">
            Disable
          </v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </v-col>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useConfigStore } from '@/store/configStore'
import { useMessageStore } from '@/store/messageStore'
import { resetAuthKey } from '@/api/config'
import { disableReadOnly } from '@/api/adminConfig'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import type { AppConfig } from '@/api/config'
import { useCurrentUser, canSeeUserPanel } from '@/script/utils/currentUser'

const readOnlyMode = defineModel<boolean>('readOnlyMode', { required: true })
const allowRegistration = defineModel<boolean>('allowRegistration', { required: true })

defineProps<{ allowRegistrationSaving?: boolean }>()
defineEmits<(e: 'update:allowRegistration', value: unknown) => void>()

const currentUser = useCurrentUser()
const isAdmin = computed(() => canSeeUserPanel(currentUser.value))

const configStore = useConfigStore('mainId')
const messageStore = useMessageStore('mainId')
const loading = ref(false)
const resetting = ref(false)
const disabling = ref(false)
const confirmOpen = ref(false)
const disablePassword = ref('')

const onReadOnlyToggle = (value: unknown) => {
  const next = value === true
  if (next) {
    readOnlyMode.value = true
    return
  }
  if (readOnlyMode.value) {
    confirmOpen.value = true
    return
  }
  readOnlyMode.value = false
}

const cancelDisable = () => {
  confirmOpen.value = false
  disablePassword.value = ''
}

const confirmDisable = async () => {
  confirmOpen.value = false
  disabling.value = true
  const password = disablePassword.value
  const success = await tryWithMessageStore('mainId', async () => {
    await disableReadOnly(password)
    readOnlyMode.value = false
    return true
  })
  if (success === true) {
    messageStore.success('Read-only mode disabled')
  }
  disablePassword.value = ''
  disabling.value = false
}

const resetKey = async () => {
  resetting.value = true
  const success = await tryWithMessageStore('mainId', async () => {
    await resetAuthKey()
    return true
  })
  if (success === true) {
    messageStore.success('Authentication key rotated — all sessions signed out')
  }
  resetting.value = false
}

const save = async () => {
  loading.value = true

  const payload: Partial<AppConfig> = {
    readOnlyMode: readOnlyMode.value
  }

  const success = await configStore.updateConfig(payload)

  if (success === true) {
    messageStore.success('Advanced settings saved successfully')
  }
  loading.value = false
}
</script>

<style scoped>
.danger-zone-card {
  border-color: rgb(var(--v-theme-error)) !important;
}
</style>
