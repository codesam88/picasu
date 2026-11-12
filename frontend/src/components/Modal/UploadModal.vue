<template>
  <v-card
    id="upload-vcard"
    class="mx-auto position-fixed"
    append-icon=""
    :title="title"
    :subtitle="subtitle"
    variant="elevated"
    retain-focus
    :style="{
      bottom: '50px',
      left: '50px',
      zIndex: 50000
    }"
  >
    <template #prepend>
      <div class="progress-container">
        <Transition name="fade-scale" mode="out-in">
          <v-progress-circular
            :key="`progress-${uploadStore.status}`"
            color="primary"
            :model-value="circularValue"
            :indeterminate="circularIndeterminate"
            class="ma-4"
          >
            <Transition name="fade-scale" mode="out-in">
              <v-icon :key="uploadStore.status" :icon="circularIcon" />
            </Transition>
          </v-progress-circular>
        </Transition>
      </div>
    </template>
    <template #append>
      <v-btn
        v-if="uploadStore.status === 'Completed' || uploadStore.status === 'Canceled'"
        variant="outlined"
        class="ma-4"
        style="width: 100px"
        @click="modalStore.showUploadModal = false"
      >
        Close
      </v-btn>
      <v-btn
        v-else-if="uploadStore.status === 'Uploading'"
        variant="outlined"
        class="ma-4"
        style="width: 100px"
        @click="uploadStore.cancelUpload()"
      >
        Cancel
      </v-btn>
    </template>
  </v-card>
</template>
<script setup lang="ts">
/**
 * This modal is used for displaying upload information.
 */
import { useModalStore } from '@/store/modalStore'
import { useUploadStore } from '@/store/uploadStore'
import humanizeDuration from 'humanize-duration'
import { computed, onUnmounted, watch } from 'vue'
import { formatBytes } from '@/script/utils/formatBytes'
const uploadStore = useUploadStore('mainId')
const modalStore = useModalStore('mainId')

const title = computed(() => {
  switch (uploadStore.status) {
    case 'Completed':
      return 'Done'
    case 'Indexing':
      return 'Indexing'
    case 'Canceled':
      return 'Canceled'
    default:
      return 'Uploading'
  }
})

const subtitle = computed(() => {
  if (uploadStore.status === 'Uploading') {
    const name = uploadStore.currentFileName ?? ''
    const index = uploadStore.currentIndex
    const count = uploadStore.fileCount
    const size = uploadStore.currentFileSize
    const sizeText = size === undefined ? '' : ` (${formatBytes(size)})`
    const remaining = humanizeDuration(uploadStore.remainingTime * 1000, {
      units: ['h', 'm', 's'],
      largest: 1,
      round: true
    })
    // Progress first, then the file being uploaded.
    return `${index}/${count} · ${name}${sizeText} · ${remaining} remaining`
  }
  if (uploadStore.status === 'Indexing') {
    const count = uploadStore.fileCount
    return `${count} ${count === 1 ? 'file' : 'files'} stored · waiting for indexing…`
  }
  return `${humanizeDuration(uploadStore.remainingTime * 1000, {
    units: ['h', 'm', 's'],
    largest: 1,
    round: true
  })} remaining`
})

const circularValue = computed(() => {
  if (uploadStore.status === 'Uploading') {
    return uploadStore.percentComplete
  } else if (uploadStore.status === 'Completed') {
    return 100
  } else {
    return undefined
  }
})

const circularIndeterminate = computed(() => uploadStore.status === 'Indexing')

const circularIcon = computed(() => {
  if (uploadStore.status === 'Completed') {
    return 'mdi-cloud-check-variant'
  } else {
    return 'mdi-cloud-upload'
  }
})

// Completed is a terminal, self-dismissing state: show the done icon briefly,
// then close the notification without a click. Any new upload reopens it.
const COMPLETED_DISMISS_MS = 1500
let dismissTimer: ReturnType<typeof setTimeout> | undefined
watch(
  () => uploadStore.status,
  (status) => {
    if (dismissTimer !== undefined) {
      clearTimeout(dismissTimer)
      dismissTimer = undefined
    }
    if (status === 'Completed') {
      dismissTimer = setTimeout(() => {
        modalStore.showUploadModal = false
        dismissTimer = undefined
      }, COMPLETED_DISMISS_MS)
    }
  }
)
onUnmounted(() => {
  if (dismissTimer !== undefined) {
    clearTimeout(dismissTimer)
    dismissTimer = undefined
  }
})
</script>
<style scoped>
.fade-scale-enter-active,
.fade-scale-leave-active {
  transition:
    opacity 0.3s ease,
    transform 0.3s ease;
}
.fade-scale-enter-from,
.fade-scale-leave-to {
  opacity: 0;
  transform: scale(0.8);
}
.fade-scale-enter-to,
.fade-scale-leave-from {
  opacity: 1;
  transform: scale(1);
}
</style>
