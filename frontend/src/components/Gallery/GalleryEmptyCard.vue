<template>
  <v-container class="d-flex align-center justify-center" fluid>
    <!-- No search results -->
    <v-row v-if="ui.isSearchEmpty" justify="center">
      <v-col class="w-100" cols="12" md="6" lg="4">
        <v-hover v-slot="{ isHovering, props: hoverProps }">
          <v-card
            class="pa-4 text-center mx-auto"
            :class="{ 'hover-cursor': ui.hasHoverEffect }"
            :style="{
              border:
                ui.hasHoverEffect && isHovering
                  ? '2px solid rgb(var(--v-border-color))'
                  : '2px solid transparent'
            }"
            :elevation="ui.hasHoverEffect && isHovering ? 12 : 2"
            rounded="lg"
            width="100%"
            v-bind="ui.hasHoverEffect ? hoverProps : {}"
          >
            <v-icon class="mb-5" color="grey" size="100"> {{ ui.icon }} </v-icon>
            <v-card-item>
              <v-card-subtitle>{{ ui.message }}</v-card-subtitle>
            </v-card-item>
          </v-card>
        </v-hover>
      </v-col>
    </v-row>

    <!-- General empty state -->
    <v-row v-else justify="center">
      <!-- Home page: dialog with upload + scan actions -->
      <template v-if="route.meta.baseName === 'timeline'">
        <v-dialog v-model="showDialog" max-width="560">
          <v-card class="pa-6">
            <v-card-title class="text-h5 font-weight-bold text-center">
              Database is empty, scan now or upload?
            </v-card-title>
            <v-card-subtitle class="text-center text-caption text-medium-emphasis mb-6">
              Library Path: {{ imagePath ?? '…' }}
            </v-card-subtitle>

            <v-row dense>
              <v-col cols="12" sm="6">
                <v-card
                  class="d-flex flex-column align-center justify-center pa-6 text-center"
                  style="cursor: pointer; min-height: 180px"
                  variant="outlined"
                  rounded="lg"
                  @click="onUploadClick"
                >
                  <v-icon size="64" color="grey">mdi-cloud-upload</v-icon>
                  <div class="text-body-1 font-weight-medium mt-3">Upload Image</div>
                </v-card>
              </v-col>

              <v-col cols="12" sm="6">
                <v-card
                  class="d-flex flex-column align-center justify-center pa-6 text-center"
                  style="cursor: pointer; min-height: 180px"
                  variant="outlined"
                  rounded="lg"
                  :disabled="!imagePath"
                  @click="onStartScan"
                >
                  <v-icon size="64" color="grey">mdi-folder-refresh-outline</v-icon>
                  <div class="text-body-1 font-weight-medium mt-3">Scan now</div>
                </v-card>
              </v-col>
            </v-row>
          </v-card>
        </v-dialog>
      </template>

      <!-- Other pages: existing layout -->
      <template v-else>
        <v-col
          v-if="ui.showUploadCard && typeof route.params.hash === 'string'"
          class="w-100"
          cols="12"
          md="6"
          lg="4"
        >
          <v-hover v-slot="{ isHovering, props: hoverProps }">
            <v-card
              class="pa-4 text-center mx-auto"
              :class="{ 'hover-cursor': true }"
              :style="{
                border: isHovering
                  ? '2px solid rgb(var(--v-border-color))'
                  : '2px solid transparent'
              }"
              :elevation="isHovering ? 12 : 2"
              rounded="lg"
              width="100%"
              v-bind="hoverProps"
              @click="uploadStore.triggerFileInput(route.params.hash)"
            >
              <v-icon class="mb-5" color="grey" size="100">mdi-cloud-upload</v-icon>
              <v-card-item>
                <v-card-subtitle>Upload new photos.</v-card-subtitle>
              </v-card-item>
            </v-card>
          </v-hover>
        </v-col>

        <v-col class="w-100" cols="12" md="6" lg="4">
          <v-hover v-slot="{ isHovering, props: hoverProps }">
            <v-card
              class="pa-4 text-center mx-auto"
              :class="{ 'hover-cursor': ui.hasHoverEffect }"
              :style="{
                border:
                  ui.hasHoverEffect && isHovering
                    ? '2px solid rgb(var(--v-border-color))'
                    : '2px solid transparent'
              }"
              :elevation="ui.hasHoverEffect && isHovering ? 12 : 2"
              rounded="lg"
              width="100%"
              v-bind="ui.hasHoverEffect ? hoverProps : {}"
              @click="ui.onClick ? ui.onClick() : undefined"
            >
              <v-icon class="mb-5" color="grey" size="100"> {{ ui.icon }} </v-icon>
              <v-card-item>
                <v-card-subtitle>{{ ui.message }}</v-card-subtitle>
              </v-card-item>
            </v-card>
          </v-hover>
        </v-col>
      </template>
    </v-row>
  </v-container>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { useCollectionStore } from '@/store/collectionStore'
import { useModalStore } from '@/store/modalStore'
import { useUploadStore } from '@/store/uploadStore'
import { useConfigStore } from '@/store/configStore'
import { startAlbumIndex } from '@/api/fs'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import type { IsolationId } from '@type/types'

const props = defineProps<{
  isolationId: IsolationId
}>()

const route = useRoute()

const uploadStore = useUploadStore('mainId')
const collectionStore = useCollectionStore(props.isolationId)
const modalStore = useModalStore('mainId')
const configStore = useConfigStore('mainId')

const showDialog = ref(true)
const imagePath = computed(() => configStore.config?.imagePath ?? null)

type ClickHandler = (() => void | Promise<void>) | undefined

interface UIState {
  isSearchEmpty: boolean
  showUploadCard: boolean
  hasHoverEffect: boolean
  message: string
  icon: string
  onClick: ClickHandler
}

const ui = computed<UIState>(() => {
  const raw = route.query.search
  const searchText = typeof raw === 'string' ? raw : Array.isArray(raw) ? raw.join(',') : ''
  const isSearching = searchText.trim() !== ''

  if (isSearching) {
    return {
      isSearchEmpty: true,
      showUploadCard: false,
      hasHoverEffect: false,
      message: 'Result not found.',
      icon: 'mdi-book-remove-multiple',
      onClick: undefined
    }
  }

  if (route.meta.baseName === 'album' && route.meta.level === 1) {
    if (collectionStore.editModeOn) {
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: false,
        message: 'All photos are already added!',
        icon: 'mdi-image-plus',
        onClick: undefined
      }
    }
    return {
      isSearchEmpty: false,
      showUploadCard: true,
      hasHoverEffect: true,
      message: 'Select from existing photos.',
      icon: 'mdi-image-plus',
      onClick: () => {
        modalStore.showHomeTempModal = true
      }
    }
  }

  switch (route.meta.baseName) {
    case 'timeline':
    case 'album':
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: true,
        message: 'Upload some photos here!',
        icon: 'mdi-image-plus',
        onClick: () => {
          uploadStore.triggerFileInput(undefined)
        }
      }

    case 'albums':
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: false,
        message: 'Albums are created automatically from your synced directory hierarchy.',
        icon: 'mdi-folder-multiple',
        onClick: undefined
      }

    case 'favorite':
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: false,
        message: 'Add your favorite photos and videos here!',
        icon: 'mdi-star',
        onClick: undefined
      }

    case 'archived':
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: false,
        message: 'Archived photos won\u2019t appear on the home page.',
        icon: 'mdi-archive-arrow-down',
        onClick: undefined
      }

    case 'trashed':
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: false,
        message: 'Deleted photos and videos appear here.',
        icon: 'mdi-delete-outline',
        onClick: undefined
      }

    case 'videos':
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: false,
        message: 'Upload some videos here!',
        icon: 'mdi-play-circle-outline',
        onClick: undefined
      }

    case 'tags':
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: false,
        message: 'Organize your photos with tags!',
        icon: 'mdi-tag-outline',
        onClick: undefined
      }

    case 'login':
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: false,
        message: 'Sign in to access your photos.',
        icon: 'mdi-login',
        onClick: undefined
      }

    default:
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: false,
        message: 'Upload some photos here!',
        icon: 'mdi-image-plus',
        onClick: undefined
      }
  }
})

function onUploadClick() {
  uploadStore.triggerFileInput(undefined)
  showDialog.value = false
}

async function onStartScan() {
  showDialog.value = false
  await tryWithMessageStore('mainId', async () => {
    await startAlbumIndex()
    return true
  })
}

onMounted(() => {
  void configStore.fetchConfig()
})
</script>

<style scoped>
.hover-cursor {
  cursor: pointer;
}
</style>
