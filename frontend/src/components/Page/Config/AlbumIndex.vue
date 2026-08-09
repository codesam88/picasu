<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">Album Index</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-list-item
        title="Scan Image Path"
        :subtitle="
          sharedNamespacePath
            ? `Index existing files under ${sharedNamespacePath} the watcher hasn't seen yet`
            : 'Configure a shared namespace first'
        "
        prepend-icon="mdi-folder-refresh-outline"
        lines="two"
      >
        <template #append>
          <v-btn
            color="primary"
            variant="flat"
            prepend-icon="mdi-magnify-scan"
            class="text-none font-weight-medium"
            :disabled="!sharedNamespacePath || isRunning"
            :loading="scanLoading"
            @click="startScan"
          >
            Scan Now
          </v-btn>
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
    </v-card>
  </v-col>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import {
  cancelAlbumIndex,
  getAlbumIndexStatus,
  startAlbumIndex,
  type AlbumIndexStatus,
  type AlbumIndexState
} from '@/api/fs'
import { useConfigStore } from '@/store/configStore'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'

const emptyStatus = (): AlbumIndexStatus => ({
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

const configStore = useConfigStore('mainId')
const messageStore = useMessageStore('mainId')

const scanLoading = ref(false)
const cancelLoading = ref(false)
const status = ref<AlbumIndexStatus>(emptyStatus())
let pollTimer: ReturnType<typeof setInterval> | undefined

const sharedNamespacePath = computed(() => {
  const shared = configStore.config?.namespaces.find((ns) => ns.name === 'shared')
  return shared?.path ?? null
})
const isRunning = computed(() => status.value.state === 'running')

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
  const result = await tryWithMessageStore('mainId', getAlbumIndexStatus)
  if (result === undefined) return
  status.value = result

  if (result.state === 'running') {
    startPolling()
  } else {
    stopPolling()
  }
}

const startScan = async () => {
  if (sharedNamespacePath.value === null) {
    messageStore.error('Configure a shared namespace before scanning')
    return
  }

  scanLoading.value = true
  const success = await tryWithMessageStore('mainId', async () => {
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
  const success = await tryWithMessageStore('mainId', async () => {
    await cancelAlbumIndex()
    return true
  })

  if (success === true) {
    messageStore.info('Index cancel requested')
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
