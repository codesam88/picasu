<template>
  <BaseModal
    v-model="modalStore.showUploadOptionsModal"
    title="Upload Options"
    width="500"
    content-class="pa-2"
    id="upload-options-modal"
  >
    <v-list lines="two" class="bg-transparent pa-0">
      <v-list-item v-for="(file, i) in uploadStore.pendingFiles" :key="`${file.name}-${i}`">
        <template #prepend>
          <v-icon icon="mdi-file-image" class="mr-2" />
        </template>
        <v-list-item-title class="text-truncate">{{ file.name }}</v-list-item-title>
        <v-list-item-subtitle>{{ formatBytes(file.size) }}</v-list-item-subtitle>
      </v-list-item>
      <v-list-item v-if="uploadStore.pendingFiles.length === 0" class="text-medium-emphasis">
        No files selected.
      </v-list-item>
    </v-list>

    <v-divider class="my-2 border-opacity-25"></v-divider>

    <v-list-item
      v-testid="'upload-auto-rename'"
      class="rounded-lg"
      :title="'Auto-rename files'"
      :subtitle="'Rename files the server considers unsafe, otherwise reject them'"
      link
      @click="uploadStore.autoRename = !uploadStore.autoRename"
    >
      <template #append>
        <v-switch
          :model-value="uploadStore.autoRename"
          color="primary"
          hide-details
          density="compact"
          inset
          readonly
          class="pointer-events-none ml-2"
        ></v-switch>
        <v-tooltip location="top" max-width="320">
          <template #activator="{ props }">
            <v-btn
              v-bind="props"
              icon="mdi-information-outline"
              variant="text"
              size="small"
              class="ml-2"
              @click.stop
            />
          </template>
          <div>
            When auto-rename is on, the server fixes unsafe filenames before saving. When off,
            uploads are rejected instead. Applies:
            <ul class="ma-0 pl-4">
              <li>remove path separators and null bytes</li>
              <li>remove characters invalid on Windows (&lt;&gt; : &quot; | ? *)</li>
              <li>prefix reserved device names (CON, PRN, AUX, …)</li>
              <li>remove zero-width and bidi-override characters</li>
              <li>normalize to Unicode NFC when enabled</li>
            </ul>
          </div>
        </v-tooltip>
      </template>
    </v-list-item>

    <template #actions>
      <v-spacer />
      <v-btn variant="text" @click="uploadStore.cancelUploadOptions()">Cancel</v-btn>
      <v-btn
        variant="tonal"
        color="primary"
        :disabled="uploadStore.pendingFiles.length === 0"
        @click="confirmUpload"
      >
        Upload
      </v-btn>
    </template>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * Pre-upload confirmation dialog: shows the picked files and lets the user
 * toggle `auto_rename` before the upload starts. Rendered by App.vue when
 * `modalStore.showUploadOptionsModal` is set (via `uploadStore.prepareUpload`).
 */
import BaseModal from './BaseModal.vue'
import { useModalStore } from '@/store/modalStore'
import { useUploadStore } from '@/store/uploadStore'

const uploadStore = useUploadStore('mainId')
const modalStore = useModalStore('mainId')

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

async function confirmUpload(): Promise<void> {
  await uploadStore.confirmUpload()
}
</script>

<style scoped>
/* Utility class to ensure the v-switch is purely visual and the interaction
   is handled by the parent v-list-item. */
.pointer-events-none {
  pointer-events: none;
}
</style>
