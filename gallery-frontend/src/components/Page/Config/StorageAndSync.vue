<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">Image Path</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-list-item
        title="Monitored Path"
        subtitle="Automatically scan changed files under this directory"
        prepend-icon="mdi-folder-network-outline"
        lines="two"
      >
        <template #append>
          <v-btn
            variant="tonal"
            prepend-icon="mdi-folder-edit-outline"
            class="text-none font-weight-medium"
            @click="showFilePicker = true"
          >
            {{ imagePath ? 'Change Path' : 'Choose Path' }}
          </v-btn>
        </template>
      </v-list-item>

      <v-divider></v-divider>
      <v-list v-if="imagePath" lines="one">
        <v-list-item :title="imagePath">
          <template #append>
            <v-btn
              icon="mdi-delete-outline"
              variant="text"
              density="comfortable"
              @click="clearPath"
              title="Clear path"
            ></v-btn>
          </template>
        </v-list-item>
      </v-list>

      <v-empty-state
        v-else
        icon="mdi-folder-open-outline"
        title="No image path set"
        text="Choose a directory to start monitoring your files. Aggregate multiple libraries under one root at the filesystem level (bind mounts or symlinks) rather than configuring several paths here."
      ></v-empty-state>

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

      <v-card-actions class="justify-end px-4 pb-4">
        <v-btn color="primary" variant="flat" :loading="loading" @click="save" class="text-none">
          Save Changes
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-col>
  <ServerFilePicker v-model="showFilePicker" @select="onFilePickerSelect" />
</template>

<script setup lang="ts">
import { ref } from 'vue'
import ServerFilePicker from './ServerFilePicker.vue'
import { useConfigStore } from '@/store/configStore'
import { useMessageStore } from '@/store/messageStore'

const imagePath = defineModel<string | null>('imagePath', { required: true })
const uploadFolder = defineModel<string>('uploadFolder', { required: true })
const configStore = useConfigStore('mainId')
const messageStore = useMessageStore('mainId')

const showFilePicker = ref(false)
const loading = ref(false)

const clearPath = () => {
  imagePath.value = null
}

const onFilePickerSelect = (path: string) => {
  imagePath.value = path || null
}

const save = async () => {
  loading.value = true
  const success = await configStore.updateConfig({
    imagePath: imagePath.value,
    uploadFolder: uploadFolder.value
  })

  if (success === true) {
    messageStore.success('Path saved successfully')
  }
  loading.value = false
}
</script>
