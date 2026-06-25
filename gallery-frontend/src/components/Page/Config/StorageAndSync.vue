<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">Image Paths</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-list-item
        title="Monitored Path"
        subtitle="Directory the backend watches for new media"
        prepend-icon="mdi-folder-network-outline"
        lines="two"
      >
        <template #append>
          <v-btn
            color="primary"
            variant="flat"
            prepend-icon="mdi-magnify-scan"
            class="text-none font-weight-medium"
            :disabled="!imagePath || isScanRunning"
            :loading="scanLoading"
            @click="startScan"
          >
            Scan Now
          </v-btn>
        </template>
      </v-list-item>

      <v-list v-if="imagePath" lines="one">
        <v-list-item :title="imagePath"></v-list-item>
      </v-list>

      <v-empty-state
        v-else
        icon="mdi-folder-open-outline"
        title="No image path set"
        text="Configure an image path in config.toml or via UROCISSA_IMAGE_HOME env var"
      ></v-empty-state>

      <v-divider></v-divider>

      <v-list-item
        title="Scan Status"
        :subtitle="statusSubtitle"
        :prepend-icon="statusIcon"
        lines="two"
      >
        <template #append>
          <v-btn
            v-if="isScanRunning"
            variant="outlined"
            color="warning"
            prepend-icon="mdi-stop"
            class="text-none"
            :loading="cancelLoading"
            :disabled="status.cancelRequested"
            @click="cancelJob"
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

      <v-divider></v-divider>

      <v-list-item
        title="Upload Folder"
        subtitle="Subfolder under Image Path that uploads with no target album land in"
        prepend-icon="mdi-tray-arrow-up"
        lines="two"
      >
        <template #append>
          <v-text-field
            v-model="uploadFolder"
            density="compact"
            variant="outlined"
            hide-details
            placeholder="uploads"
            style="max-width: 160px"
          ></v-text-field>
        </template>
      </v-list-item>

      <v-divider></v-divider>

      <v-list-item
        title="Max Upload Size"
        subtitle="Maximum size for a single file upload (e.g. 500MiB, 1GiB)"
        prepend-icon="mdi-upload-lock-outline"
        lines="two"
      >
        <template #append>
          <v-text-field
            v-model="maxUploadSize"
            density="compact"
            variant="outlined"
            hide-details
            placeholder="100MiB"
            style="max-width: 160px"
          ></v-text-field>
        </template>
      </v-list-item>

      <v-card-actions class="justify-end px-4 pb-4">
        <v-btn color="primary" variant="flat" :loading="saving" @click="save" class="text-none">
          Save Changes
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-col>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useConfigStore } from '@/store/configStore'
import { useMessageStore } from '@/store/messageStore'
import {
  cancelAlbumIndex,
  getAlbumIndexStatus,
  startAlbumIndex,
  type AlbumIndexStatus,
  type AlbumIndexState
} from '@/api/fs'

const props = defineProps<{ imagePath: string | null }>()
const uploadFolder = defineModel<string>('uploadFolder', { required: true })
const maxUploadSize = defineModel<string>('maxUploadSize', { required: true })
const configStore = useConfigStore('mainId')
const messageStore = useMessageStore('mainId')

const saving = ref(false)
const scanLoading = ref(false)
const cancelLoading = ref(false)
const status = ref<AlbumIndexStatus>(emptyStatus())
let pollTimer: ReturnType<typeof setInterval> | undefined

const isScanRunning = computed(() => status.value.state === 'running')

const counters = computed(() => [
  { label: 'Scanned', value: status.value.scanned },
  { label: 'Matched', value: status.value.matched },
  { label: 'Processed', value: status.value.processed },
  { label: 'Failed', value: status.value.failed }
])

const stateLabel: Record<AlbumIndexState, string> = {
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
  if (status.value.state === 'idle') return 'No index has run yet'
  if (status.value.state === 'running') {
    return status.value.cancelRequested ? `Canceling ${root ?? ''}` : `Scanning ${root ?? ''}`
  }
  const finished = formatTime(status.value.finishedAt)
  return finished ? `${label} at ${finished}` : label
})

function emptyStatus(): AlbumIndexStatus {
  return {
    state: 'idle',
    root: null,
    scanned: 0,
    matched: 0,
    processed: 0,
    failed: 0,
    startedAt: null,
    finishedAt: null,
    cancelRequested: false
  }
}

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
  const result = await tryWithMsg(getAlbumIndexStatus)
  if (result === undefined) return
  status.value = result
  if (result.state === 'running') {
    startPolling()
  } else {
    stopPolling()
  }
}

const startScan = async () => {
  if (props.imagePath === null) {
    messageStore.error('Set an Image Path before scanning')
    return
  }
  scanLoading.value = true
  const success = await tryWithMsg(async () => {
    await startAlbumIndex()
    return true
  })
  if (success === true) {
    messageStore.success('Album index started')
    await refreshStatus()
  }
  scanLoading.value = false
}

const cancelJob = async () => {
  cancelLoading.value = true
  const success = await tryWithMsg(async () => {
    await cancelAlbumIndex()
    return true
  })
  if (success === true) {
    messageStore.info('Index cancel requested')
    await refreshStatus()
  }
  cancelLoading.value = false
}

const save = async () => {
  saving.value = true
  const success = await configStore.updateConfig({
    uploadFolder: uploadFolder.value,
    maxUploadSize: maxUploadSize.value
  })
  if (success === true) {
    messageStore.success('Path settings saved successfully')
  }
  saving.value = false
}

async function tryWithMsg<T>(fn: () => Promise<T>): Promise<T | undefined> {
  const { tryWithMessageStore } = await import('@/script/utils/try_catch')
  return tryWithMessageStore('mainId', fn)
}

onMounted(() => {
  void refreshStatus()
})

onBeforeUnmount(() => {
  stopPolling()
})
</script>
