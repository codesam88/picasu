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
      <!-- Home page: inline actions, part of the page (no modal) -->
      <template v-if="route.meta.baseName === 'timeline'">
        <v-col cols="12" md="8" lg="6" class="mx-auto text-center">
          <div class="text-h6 font-weight-medium mb-4">Database is empty, scan now or upload?</div>
          <v-row dense justify="center">
            <v-col cols="12" sm="6">
              <v-card
                class="d-flex flex-column align-center justify-center pa-6 text-center"
                style="cursor: pointer; min-height: 180px"
                :elevation="2"
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
                :elevation="2"
                rounded="lg"
                :disabled="!canScan"
                @click="onStartScan"
              >
                <v-icon size="64" color="grey">mdi-folder-refresh-outline</v-icon>
                <div class="text-body-1 font-weight-medium mt-3">Scan now</div>
              </v-card>
            </v-col>
          </v-row>
        </v-col>
      </template>

      <!-- Other pages: existing layout -->
      <template v-else>
        <v-col
          v-if="ui.showUploadCard && typeof route.params.assetId === 'string'"
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
              @click="uploadStore.triggerFileInput(route.params.assetId)"
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
import { computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useUploadStore } from '@/store/uploadStore'
import { useConfigStore } from '@/store/configStore'
import { useCurrentUser } from '@/script/utils/currentUser'
import { startAlbumIndex } from '@/api/fs'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import type { IsolationId } from '@type/types'

defineProps<{
  isolationId: IsolationId
}>()

const route = useRoute()

const uploadStore = useUploadStore('mainId')
const configStore = useConfigStore('mainId')

// Scanning rewrites the shared index: admin-only. Viewers get the upload
// card instead of a button that would only answer 401.
const canScan = computed(() => useCurrentUser().value?.isAdmin === true)

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
    return {
      isSearchEmpty: false,
      showUploadCard: false,
      hasHoverEffect: false,
      message: 'This album is empty.',
      icon: 'mdi-folder-open-outline',
      onClick: undefined
    }
  }

  switch (route.meta.baseName) {
    case 'timeline':
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

    case 'album':
      return {
        isSearchEmpty: false,
        showUploadCard: false,
        hasHoverEffect: false,
        message: 'This album is empty.',
        icon: 'mdi-folder-open-outline',
        onClick: undefined
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
}

async function onStartScan() {
  if (!canScan.value) return
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
