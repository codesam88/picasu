<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold"> Advanced Settings </v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-list-item
        title="Read Only Mode"
        subtitle="Prevent modification of data and settings"
        @click="readOnlyMode = !readOnlyMode"
      >
        <template #append>
          <v-switch
            v-model="readOnlyMode"
            color="primary"
            hide-details
            inset
            @click.stop
          ></v-switch>
        </template>
      </v-list-item>

      <v-divider></v-divider>

      <v-list-item
        title="Disable Processing"
        subtitle="Skip frontend image rendering for debugging"
        @click="disableImg = !disableImg"
      >
        <template #append>
          <v-switch v-model="disableImg" color="primary" inset @click.stop></v-switch>
        </template>
      </v-list-item>

      <v-divider></v-divider>
      <v-list-item
        title="JWT Authentication Key"
        :subtitle="
          hasAuthKey
            ? 'Configured — resetting signs everyone out'
            : 'Not configured — random key per process restart'
        "
      >
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
  </v-col>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useConfigStore } from '@/store/configStore'
import { useMessageStore } from '@/store/messageStore'
import { resetAuthKey } from '@/api/config'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import type { AppConfig } from '@/api/config'

const readOnlyMode = defineModel<boolean>('readOnlyMode', { required: true })
const disableImg = defineModel<boolean>('disableImg', { required: true })
const hasAuthKey = defineModel<boolean>('hasAuthKey', { required: true })

const configStore = useConfigStore('mainId')
const messageStore = useMessageStore('mainId')
const loading = ref(false)
const resetting = ref(false)

const resetKey = async () => {
  resetting.value = true
  const success = await tryWithMessageStore('mainId', async () => {
    const result = await resetAuthKey()
    hasAuthKey.value = result.hasAuthKey
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
    readOnlyMode: readOnlyMode.value,
    disableImg: disableImg.value
  }

  const success = await configStore.updateConfig(payload)

  if (success === true) {
    messageStore.success('Advanced settings saved successfully')
  }
  loading.value = false
}
</script>
