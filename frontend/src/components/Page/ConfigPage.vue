<template>
  <PageTemplate
    preset="card"
    width="narrow"
    card-class="config-page-surface bg-transparent overflow-y-auto w-100"
    :ready="initializedStore.initialized"
  >
    <template #content>
      <div class="config-stack">
        <v-tabs v-model="activeTab" color="primary" class="mb-2">
          <v-tab value="server">Server</v-tab>
          <v-tab v-if="isAdmin" value="users">Users</v-tab>
          <v-tab v-if="isAdmin" value="invites">Invites</v-tab>
          <v-tab v-if="isAdmin" value="shares">Shares</v-tab>
        </v-tabs>

        <v-tabs-window v-model="activeTab">
          <v-tabs-window-item value="server">
            <div class="config-block">
              <v-row class="ma-0">
                <StorageAndSync
                  :image-path="imagePath"
                  :users-home-path="usersHomePath"
                  :personal-albums-enabled="personalAlbumsEnabled"
                  v-model:upload-folder="localSettings.uploadFolder"
                  v-model:max-upload-size="localSettings.maxUploadSize"
                  v-model:fs-notify-watcher="localSettings.fsNotifyWatcher"
                />
              </v-row>
            </div>

            <div class="config-block">
              <v-row class="ma-0">
                <AdvancedConfig
                  v-model:read-only-mode="localSettings.readOnlyMode"
                  v-model:allow-registration="allowRegistration"
                  :allow-registration-saving="allowRegistrationSaving"
                  @update:allow-registration="onAllowRegistrationChange"
                />
              </v-row>
            </div>

            <div v-if="isAdmin" class="config-block">
              <v-row class="ma-0">
                <v-col cols="12">
                  <v-card border flat>
                    <v-card-title class="font-weight-bold">Export</v-card-title>
                    <v-divider thickness="4" variant="double"></v-divider>
                    <v-card-text class="text-medium-emphasis">
                      Download the current server configuration as JSON.
                    </v-card-text>
                    <v-card-actions class="justify-end px-4 pb-4">
                      <v-btn
                        color="primary"
                        variant="flat"
                        :loading="exporting"
                        @click="exportConfig"
                        class="text-none"
                      >
                        Export Config
                      </v-btn>
                    </v-card-actions>
                  </v-card>
                </v-col>
              </v-row>
            </div>
          </v-tabs-window-item>

          <v-tabs-window-item v-if="isAdmin" value="users">
            <div class="config-block">
              <v-row class="ma-0">
                <UserManagement />
              </v-row>
            </div>
          </v-tabs-window-item>

          <v-tabs-window-item v-if="isAdmin" value="invites">
            <div class="config-block">
              <v-row class="ma-0">
                <InviteManagement />
              </v-row>
            </div>
          </v-tabs-window-item>

          <v-tabs-window-item v-if="isAdmin" value="shares">
            <div class="config-block">
              <v-row class="ma-0">
                <ShareManagement />
              </v-row>
            </div>
          </v-tabs-window-item>
        </v-tabs-window>
      </div>
    </template>
  </PageTemplate>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted, onBeforeUnmount } from 'vue'
import { useConfigStore } from '@/store/configStore'
import { useInitializedStore } from '@/store/initializedStore'
import { useMessageStore } from '@/store/messageStore'
import type { AppConfig } from '@/api/config'
import { getAdminConfig, updateAdminConfig, exportAdminConfig } from '@/api/adminConfig'
import PageTemplate from './PageLayout/PageTemplate.vue'
import StorageAndSync from './Config/StorageAndSync.vue'
import AdvancedConfig from './Config/AdvancedConfig.vue'
import UserManagement from './Config/UserManagement.vue'
import InviteManagement from './Config/InviteManagement.vue'
import ShareManagement from './Config/ShareManagement.vue'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { useCurrentUser, canSeeUserPanel } from '@/script/utils/currentUser'

const configStore = useConfigStore('mainId')
const initializedStore = useInitializedStore('mainId')
const messageStore = useMessageStore('mainId')
const currentUser = useCurrentUser()
const isAdmin = computed(() => canSeeUserPanel(currentUser.value))

// UI State
const loading = ref(false)
const activeTab = ref('server')
const allowRegistration = ref(false)
const allowRegistrationSaving = ref(false)
const personalAlbumsEnabled = ref(false)
const usersHomePath = ref<string | null>(null)
const exporting = ref(false)

// Local State
const imagePath = ref<string | null>(null)

const localSettings = reactive<AppConfig>({
  readOnlyMode: false,
  fsNotifyWatcher: true,
  authKey: '',
  uploadFolder: 'uploads',
  maxUploadSize: '100MiB',

  limits: {}
})

const syncLocalWithStore = () => {
  if (configStore.config) {
    Object.assign(localSettings, JSON.parse(JSON.stringify(configStore.config)))
  }
}

const loadAdminConfig = async () => {
  if (!isAdmin.value) return
  const entries = await getAdminConfig()
  const field = entries.find((e) => e.name === 'allow_registration')
  if (field) {
    allowRegistration.value = field.value === true
  }
  const paField = entries.find((e) => e.name === 'personal_albums_enabled')
  if (paField) {
    personalAlbumsEnabled.value = paField.value === true
  }
  const imageHome = entries.find((e) => e.name === 'image_home')
  if (typeof imageHome?.value === 'string') {
    imagePath.value = imageHome.value
  }
  const usersHome = entries.find((e) => e.name === 'users_home')
  if (typeof usersHome?.value === 'string') {
    usersHomePath.value = usersHome.value
  } else {
    usersHomePath.value = null
  }
}

const onAllowRegistrationChange = async (value: unknown) => {
  const next = value === true
  const previous = allowRegistration.value
  allowRegistration.value = next
  allowRegistrationSaving.value = true
  const success = await tryWithMessageStore('mainId', async () => {
    await updateAdminConfig({ allowRegistration: next })
    return true
  })
  allowRegistrationSaving.value = false
  if (success === true) {
    messageStore.success('Registration setting saved')
  } else {
    allowRegistration.value = previous
  }
}

const exportConfig = async () => {
  exporting.value = true
  const result = await tryWithMessageStore('mainId', async () => {
    const data = await exportAdminConfig()
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = 'picasu-config.json'
    a.click()
    URL.revokeObjectURL(url)
    return true
  })
  if (result === true) {
    messageStore.success('Config exported')
  }
  exporting.value = false
}

const initData = async () => {
  loading.value = true
  const result = await tryWithMessageStore('mainId', async () => {
    // Always re-fetch from backend so unsaved changes from a previous visit
    // don't persist in the displayed state.
    configStore.$patch({ config: undefined })
    await configStore.fetchConfig()
    syncLocalWithStore()
    await loadAdminConfig()
    return true
  })

  if (result === true) {
    initializedStore.initialized = true
  }

  loading.value = false
}

onMounted(initData)

onBeforeUnmount(() => {
  initializedStore.initialized = false
})
</script>

<style>
.config-stack {
  background-color: transparent;
}

.config-block {
  padding-bottom: 24px;
}

.config-block:last-child {
  padding-bottom: 0;
}

.config-block :deep(.v-col) {
  padding-bottom: 0;
  padding-top: 0;
}

.config-page-surface {
  background: transparent !important;
}
</style>
