<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">One-Time Import</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-list-item
        title="Import Folder"
        :subtitle="selectedPath || 'No folder selected'"
        prepend-icon="mdi-folder-arrow-down-outline"
        lines="two"
      >
        <template #append>
          <div class="d-flex ga-2 flex-wrap justify-end">
            <v-btn
              variant="tonal"
              prepend-icon="mdi-folder-search-outline"
              class="text-none font-weight-medium"
              :disabled="isRunning"
              @click="showFilePicker = true"
            >
              Browse
            </v-btn>
            <v-btn
              color="primary"
              variant="flat"
              prepend-icon="mdi-play"
              class="text-none font-weight-medium"
              :disabled="!selectedPath || isRunning"
              :loading="loading"
              @click="startImport"
            >
              Scan Once
            </v-btn>
          </div>
        </template>
      </v-list-item>

      <v-divider></v-divider>

      <v-list-item title="Status" :subtitle="statusSubtitle" :prepend-icon="statusIcon" lines="two">
        <template #append>
          <v-btn
            v-if="isRunning"
            variant="outlined"
            color="warning"
            prepend-icon="mdi-stop"
            class="text-none"
            :loading="cancelLoading"
            :disabled="status.cancelRequested"
            @click="cancelImport"
          >
            Cancel
          </v-btn>
        </template>
      </v-list-item>

      <v-row class="ma-0 px-4 pb-4" dense>
        <v-col v-for="counter in counters" :key="counter.label" cols="6" sm="3">
          <v-sheet border rounded class="pa-3">
            <div class="text-caption text-medium-emphasis">{{ counter.label }}</div>
            <div class="text-h6">{{ counter.value }}</div>
          </v-sheet>
        </v-col>
      </v-row>
    </v-card>
  </v-col>

  <ServerFilePicker
    v-model="showFilePicker"
    :initial-path="selectedPath"
    @select="onFilePickerSelect"
  />
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import {
  cancelFolderImport,
  getFolderImportStatus,
  startFolderImport,
  type FolderImportStatus,
  type FolderImportState
} from '@/api/fs'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import ServerFilePicker from './ServerFilePicker.vue'

const emptyStatus = (): FolderImportStatus => ({
  state: 'idle',
  root: null,
  scanned: 0,
  matched: 0,
  processed: 0,
  failed: 0,
  startedAt: null,
  finishedAt: null,
  cancelRequested: false
})

const messageStore = useMessageStore('mainId')

const selectedPath = ref('')
const showFilePicker = ref(false)
const loading = ref(false)
const cancelLoading = ref(false)
const status = ref<FolderImportStatus>(emptyStatus())
let pollTimer: ReturnType<typeof setInterval> | undefined

const isRunning = computed(() => status.value.state === 'running')

const counters = computed(() => [
  { label: 'Scanned', value: status.value.scanned },
  { label: 'Matched', value: status.value.matched },
  { label: 'Processed', value: status.value.processed },
  { label: 'Failed', value: status.value.failed }
])

const stateLabel: Record<FolderImportState, string> = {
  idle: 'Idle',
  running: 'Running',
  completed: 'Completed',
  canceled: 'Canceled',
  failed: 'Failed'
}

const statusIcon = computed(() => {
  if (status.value.state === 'running') return 'mdi-progress-clock'
  if (status.value.state === 'completed') return 'mdi-check-circle-outline'
  if (status.value.state === 'canceled') return 'mdi-cancel'
  if (status.value.state === 'failed') return 'mdi-alert-circle-outline'
  return 'mdi-timer-sand-empty'
})

const formatTime = (value: number | null) => {
  if (value === null) return ''
  return new Date(value).toLocaleString()
}

const statusSubtitle = computed(() => {
  const root = status.value.root
  const label = stateLabel[status.value.state]
  if (status.value.state === 'idle') return 'No import has run yet'
  if (status.value.state === 'running') {
    return status.value.cancelRequested ? `Canceling ${root ?? ''}` : `Scanning ${root ?? ''}`
  }

  const finished = formatTime(status.value.finishedAt)
  return finished ? `${label} at ${finished}` : label
})

const stopPolling = () => {
  if (pollTimer !== undefined) {
    clearInterval(pollTimer)
    pollTimer = undefined
  }
}

const startPolling = () => {
  if (pollTimer !== undefined) return
  pollTimer = setInterval(() => {
    void refreshStatus()
  }, 1500)
}

const refreshStatus = async () => {
  const result = await tryWithMessageStore('mainId', getFolderImportStatus)
  if (result === undefined) return
  status.value = result

  if (result.state === 'running') {
    startPolling()
  } else {
    stopPolling()
  }
}

const onFilePickerSelect = (path: string) => {
  selectedPath.value = path
}

const startImport = async () => {
  if (!selectedPath.value) {
    messageStore.error('Select a folder before starting import')
    return
  }

  loading.value = true
  const success = await tryWithMessageStore('mainId', async () => {
    await startFolderImport(selectedPath.value)
    return true
  })

  if (success === true) {
    messageStore.success('Folder import started')
    await refreshStatus()
  }

  loading.value = false
}

const cancelImport = async () => {
  cancelLoading.value = true
  const success = await tryWithMessageStore('mainId', async () => {
    await cancelFolderImport()
    return true
  })

  if (success === true) {
    messageStore.info('Folder import cancel requested')
    await refreshStatus()
  }

  cancelLoading.value = false
}

onMounted(() => {
  void refreshStatus()
})

onBeforeUnmount(() => {
  stopPolling()
})
</script>
