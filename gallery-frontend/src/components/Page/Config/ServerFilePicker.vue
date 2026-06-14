<template>
  <v-dialog
    v-model="modelValue"
    :fullscreen="isMobile"
    :max-width="isMobile ? undefined : 600"
    :height="isMobile ? undefined : 600"
    scrollable
    transition="dialog-bottom-transition"
  >
    <v-card :height="isMobile ? '100%' : 600" class="d-flex flex-column">
      <div class="folder-picker-header border-b">
        <div class="d-flex align-center ga-2 px-3 pt-3">
          <v-btn
            icon="mdi-arrow-left"
            variant="text"
            density="comfortable"
            title="Go up"
            :disabled="!currentPath"
            @click="navigateUp"
          />

          <div class="path-breadcrumbs flex-grow-1 min-w-0">
            <template v-if="breadcrumbs.length > 0">
              <template
                v-for="(item, index) in breadcrumbDisplayItems"
                :key="getBreadcrumbDisplayKey(item, index)"
              >
                <v-menu v-if="item.kind === 'overflow'" location="bottom start">
                  <template #activator="{ props: menuProps }">
                    <v-btn
                      v-bind="menuProps"
                      icon="mdi-dots-horizontal"
                      variant="text"
                      density="comfortable"
                      size="small"
                      title="Show hidden folders"
                    />
                  </template>

                  <v-list density="compact" class="breadcrumb-menu">
                    <v-list-item
                      v-for="crumb in hiddenBreadcrumbs"
                      :key="crumb.path"
                      :title="crumb.label"
                      :subtitle="crumb.path"
                      @click="navigateToPath(crumb.path)"
                    />
                  </v-list>
                </v-menu>

                <v-btn
                  v-else
                  variant="text"
                  size="small"
                  :class="[
                    'breadcrumb-btn',
                    'text-none',
                    { 'breadcrumb-btn--fixed': isFixedBreadcrumb(item.crumb) }
                  ]"
                  :title="item.crumb.path"
                  @click="navigateToPath(item.crumb.path)"
                >
                  {{ item.crumb.label }}
                </v-btn>

                <v-icon
                  v-if="index < breadcrumbDisplayItems.length - 1"
                  icon="mdi-chevron-right"
                  size="small"
                  class="text-medium-emphasis"
                />
              </template>
            </template>
            <v-btn
              v-else
              variant="text"
              size="small"
              class="breadcrumb-btn text-none"
              @click="navigateToPath('')"
            >
              Root
            </v-btn>
          </div>

          <v-btn
            icon="mdi-close"
            variant="text"
            density="comfortable"
            title="Close"
            @click="modelValue = false"
          />
        </div>

        <div class="px-3 pb-3 pt-2">
          <v-text-field
            v-model="currentPath"
            placeholder="Path..."
            hide-details
            density="compact"
            variant="outlined"
            single-line
            :error="!!errorMsg"
            @keyup.enter="loadCurrentPath"
          >
            <template #append-inner>
              <v-fade-transition>
                <v-icon
                  v-if="currentPath"
                  icon="mdi-arrow-right"
                  class="cursor-pointer"
                  @click="loadCurrentPath"
                />
              </v-fade-transition>
            </template>
          </v-text-field>
        </div>
      </div>

      <v-card-text class="pa-0 d-flex flex-column">
        <v-list v-if="loading" disabled>
          <v-skeleton-loader type="list-item@5" />
        </v-list>

        <v-empty-state
          v-else-if="items.length === 0 && roots.length === 0"
          :icon="emptyStateIcon"
          title="No folders found"
          :text="errorMsg || 'This folder has no subfolders.'"
          class="ma-auto"
        />

        <v-list v-else lines="one" density="default">
          <template v-if="isDefault && roots.length > 0">
            <v-list-subheader>Drives / Roots</v-list-subheader>
            <v-list-item
              v-for="item in roots"
              :key="item"
              :value="item"
              color="primary"
              @click="navigateDown(item)"
            >
              <template #prepend>
                <v-icon icon="mdi-harddisk" />
              </template>
              <v-list-item-title>{{ item }}</v-list-item-title>
              <template #append>
                <v-icon icon="mdi-chevron-right" size="small" />
              </template>
            </v-list-item>
            <v-divider class="my-2" />
            <v-list-subheader>Current Directory</v-list-subheader>
          </template>

          <v-list-item
            v-for="item in items"
            :key="item"
            :value="item"
            color="primary"
            @click="navigateDown(item)"
          >
            <template #prepend>
              <v-icon icon="mdi-folder" />
            </template>

            <v-list-item-title>
              {{ getFolderName(item) }}
            </v-list-item-title>

            <template #append>
              <v-icon icon="mdi-chevron-right" size="small" />
            </template>
          </v-list-item>
        </v-list>
      </v-card-text>

      <v-divider />

      <v-card-actions class="folder-picker-actions">
        <div class="selected-path text-body-small text-medium-emphasis">
          Selected:
          <span class="text-high-emphasis">{{ selectedFolderLabel }}</span>
        </div>

        <v-btn variant="tonal" class="text-none" @click="confirmSelection" :disabled="!currentPath">
          Select Folder
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useDisplay } from 'vuetify'
import { fetchFsCompletion } from '@/api/fs'
import axios from 'axios'

interface Breadcrumb {
  label: string
  path: string
}

type BreadcrumbDisplayItem =
  | {
      kind: 'crumb'
      crumb: Breadcrumb
    }
  | {
      kind: 'overflow'
    }

// --- Props & Emits ---
const modelValue = defineModel<boolean>({ required: true })

const props = defineProps<{
  initialPath?: string
}>()

const emit = defineEmits<(e: 'select', path: string) => void>()

// --- Responsiveness ---
const { mobile } = useDisplay()
const isMobile = computed(() => mobile.value)

// --- State ---
const currentPath = ref('')
const items = ref<string[]>([])
const roots = ref<string[]>([])
const isDefault = ref(false)
const loading = ref(false)
const errorMsg = ref('')

const emptyStateIcon = computed(() => {
  if (errorMsg.value) return 'mdi-folder-alert-outline'
  return 'mdi-folder-open-outline'
})

const selectedPathLabel = computed(() => currentPath.value || 'Root')
const selectedFolderLabel = computed(
  () => getFolderName(currentPath.value) || selectedPathLabel.value
)

const breadcrumbs = computed<Breadcrumb[]>(() => buildBreadcrumbs(currentPath.value))
const hiddenBreadcrumbs = computed<Breadcrumb[]>(() => {
  if (breadcrumbs.value.length <= 5) return []
  return breadcrumbs.value.slice(1, -3)
})

const breadcrumbDisplayItems = computed<BreadcrumbDisplayItem[]>(() => {
  if (hiddenBreadcrumbs.value.length === 0) {
    return breadcrumbs.value.map((crumb) => ({ kind: 'crumb' as const, crumb }))
  }

  const firstCrumb = breadcrumbs.value[0]
  if (firstCrumb === undefined) return []

  return [
    { kind: 'crumb', crumb: firstCrumb },
    { kind: 'overflow' },
    ...breadcrumbs.value.slice(-3).map((crumb) => ({ kind: 'crumb' as const, crumb }))
  ]
})

// Utilities
const getBreadcrumbDisplayKey = (item: BreadcrumbDisplayItem, index: number) => {
  return item.kind === 'crumb' ? item.crumb.path : `overflow-${index}`
}

const isFixedBreadcrumb = (crumb: Breadcrumb) =>
  /^[A-Za-z]:\\$/.test(crumb.label) || crumb.label === '/'

const getFolderName = (fullPath: string) => {
  if (!fullPath) return ''
  const separator = fullPath.includes('\\') ? '\\' : '/'
  const trimmedPath = fullPath.endsWith(separator) ? fullPath.slice(0, -1) : fullPath
  if (!trimmedPath) return fullPath
  if (/^[A-Za-z]:$/.test(trimmedPath)) return `${trimmedPath}\\`
  // eslint-disable-next-line @typescript-eslint/prefer-nullish-coalescing, @typescript-eslint/strict-boolean-expressions
  return trimmedPath.split(separator).pop() || fullPath
}

const ensureTrailingSeparator = (path: string) => {
  if (!path) return ''
  const isWindows = path.includes('\\')
  const separator = isWindows ? '\\' : '/'
  return path.endsWith(separator) ? path : `${path}${separator}`
}

const appendTrailingSeparator = (path: string, separator: '\\' | '/') => {
  return path.endsWith(separator) ? path : `${path}${separator}`
}

const buildWindowsBreadcrumbs = (path: string): Breadcrumb[] => {
  const normalized = path.replace(/\//g, '\\')
  const driveMatch = /^[A-Za-z]:\\?/.exec(normalized)
  const crumbs: Breadcrumb[] = []

  if (driveMatch !== null) {
    const drive = driveMatch[0].endsWith('\\') ? driveMatch[0] : `${driveMatch[0]}\\`
    crumbs.push({ label: drive, path: drive })

    const rest = normalized.slice(drive.length)
    const parts = rest.split('\\').filter((part) => part.length > 0)
    let accumulated = drive

    for (const part of parts) {
      accumulated = appendTrailingSeparator(`${accumulated}${part}`, '\\')
      crumbs.push({ label: part, path: accumulated })
    }

    return crumbs
  }

  return buildRelativeBreadcrumbs(normalized, '\\')
}

const buildUnixBreadcrumbs = (path: string): Breadcrumb[] => {
  if (path.startsWith('/')) {
    const crumbs: Breadcrumb[] = [{ label: '/', path: '/' }]
    const parts = path
      .slice(1)
      .split('/')
      .filter((part) => part.length > 0)
    let accumulated = '/'

    for (const part of parts) {
      accumulated = appendTrailingSeparator(`${accumulated}${part}`, '/')
      crumbs.push({ label: part, path: accumulated })
    }

    return crumbs
  }

  return buildRelativeBreadcrumbs(path, '/')
}

const buildRelativeBreadcrumbs = (path: string, separator: '\\' | '/') => {
  const parts = path.split(separator).filter((part) => part.length > 0)
  const crumbs: Breadcrumb[] = []
  let accumulated = ''

  for (const part of parts) {
    accumulated = appendTrailingSeparator(accumulated ? `${accumulated}${part}` : part, separator)
    crumbs.push({ label: part, path: accumulated })
  }

  return crumbs
}

const buildBreadcrumbs = (path: string) => {
  if (!path) return []
  return path.includes('\\') ? buildWindowsBreadcrumbs(path) : buildUnixBreadcrumbs(path)
}

// --- Logic ---
const loadItems = async (path: string) => {
  loading.value = true
  errorMsg.value = ''
  try {
    const res = await fetchFsCompletion(path)
    items.value = res.children
    roots.value = res.roots
    isDefault.value = res.is_default
  } catch (e: unknown) {
    console.error(e)
    items.value = []
    roots.value = []
    isDefault.value = false
    if (axios.isAxiosError(e) && e.response?.status === 404) {
      errorMsg.value = 'Directory does not exist'
    } else {
      errorMsg.value = 'Error listing directory'
    }
  } finally {
    loading.value = false
  }
}

const loadCurrentPath = () => {
  loadItems(currentPath.value).catch(console.error)
}

const navigateToPath = (path: string) => {
  currentPath.value = ensureTrailingSeparator(path)
  loadItems(currentPath.value).catch(console.error)
}

const navigateDown = (path: string) => {
  navigateToPath(path)
}

const navigateUp = () => {
  if (!currentPath.value) return

  const isWindows = currentPath.value.includes('\\')
  const separator = isWindows ? '\\' : '/'

  // clean up existing path to handle parsing
  const cleanPath = currentPath.value.endsWith(separator)
    ? currentPath.value.slice(0, -1)
    : currentPath.value
  const parts = cleanPath.split(separator)

  // Go to root logic
  if (parts.length <= 1) {
    currentPath.value = ''
  } else {
    parts.pop() // Remove last segment

    if (isWindows) {
      // e.g. "C:" needs backslash to be valid root often
      currentPath.value = parts.join('\\') + (parts.length === 1 ? '\\' : '')
    } else {
      // e.g. "" -> join -> "" implies root /
      const newPath = parts.join('/')
      currentPath.value = newPath || '/'
    }

    // Ensure trailing slash for intermediate directories to avoid "searching" mode
    if (!currentPath.value.endsWith(separator)) {
      currentPath.value += separator
    }
  }

  loadItems(currentPath.value).catch(console.error)
}

const confirmSelection = () => {
  if (currentPath.value) {
    let selected = currentPath.value
    const isWindows = selected.includes('\\')
    const separator = isWindows ? '\\' : '/'

    // Normalize root check
    const isRoot = (isWindows && selected.length <= 3) || (!isWindows && selected === '/')

    // Remove trailing slash if not root
    if (!isRoot && selected.endsWith(separator)) {
      selected = selected.slice(0, -1)
    }

    emit('select', selected)
    modelValue.value = false
  }
}

// --- Watchers ---
watch(modelValue, (isOpen) => {
  if (isOpen) {
    currentPath.value = props.initialPath ?? ''
    loadItems(currentPath.value).catch((err: unknown) => {
      console.error('Failed to load items in watcher:', err)
    })
  }
})
</script>

<style scoped>
.folder-picker-header {
  flex: 0 0 auto;
}

.folder-picker-actions {
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 18px 24px;
}

.selected-path {
  flex: 1 1 auto;
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.path-breadcrumbs {
  align-items: center;
  display: flex;
  min-height: 36px;
  overflow: hidden;
  scrollbar-width: none;
  white-space: nowrap;
}

.path-breadcrumbs::-webkit-scrollbar {
  display: none;
}

.breadcrumb-btn {
  flex: 0 0 auto;
  max-width: 180px;
}

.breadcrumb-btn--fixed {
  max-width: none;
}

.breadcrumb-btn :deep(.v-btn__content) {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.breadcrumb-btn--fixed :deep(.v-btn__content) {
  overflow: visible;
  text-overflow: clip;
}

.breadcrumb-menu {
  max-width: 420px;
}
</style>
