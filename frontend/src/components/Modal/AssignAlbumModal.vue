<template>
  <v-dialog v-model="modalStore.showAssignAlbumModal" persistent max-width="480" scrollable>
    <v-card>
      <v-card-title class="d-flex align-center gap-2 pt-4 pb-2 px-4">
        <v-icon>mdi-folder-move</v-icon>
        Move to Album
      </v-card-title>

      <v-divider />

      <v-card-text class="pa-0" style="max-height: 60vh; overflow-y: auto">
        <!-- Search -->
        <div class="px-4 pt-3 pb-1">
          <v-text-field
            v-model="search"
            density="compact"
            variant="outlined"
            prepend-inner-icon="mdi-magnify"
            label="Search albums"
            clearable
            hide-details
            autofocus
          />
        </div>

        <!-- Album tree -->
        <v-list density="compact" class="pa-0">
          <template v-for="node in flatTree" :key="node.id">
            <v-list-item
              v-if="node.visible"
              :style="{ paddingLeft: `${node.depth * 20 + 12}px` }"
              :class="{ 'bg-primary-lighten-4': node.id === selectedAlbumId }"
              :value="node.id"
              @click="selectedAlbumId = node.id"
            >
              <template #prepend>
                <v-icon size="small" class="mr-2">
                  {{ node.id === selectedAlbumId ? 'mdi-folder-open' : 'mdi-folder' }}
                </v-icon>
              </template>
              <v-list-item-title>
                {{ node.title }}
                <v-chip
                  v-if="node.id === currentAlbumId"
                  size="x-small"
                  color="secondary"
                  class="ml-2"
                  >current</v-chip
                >
              </v-list-item-title>
              <v-list-item-subtitle v-if="node.dirPath" class="text-caption text-truncate">
                {{ node.dirPath }}
              </v-list-item-subtitle>
              <template #append v-if="node.id === selectedAlbumId">
                <v-icon color="primary" size="small">mdi-check</v-icon>
              </template>
            </v-list-item>
          </template>

          <v-list-item v-if="visibleCount === 0" class="text-medium-emphasis text-caption">
            No albums match your search.
          </v-list-item>
        </v-list>

        <v-divider class="mt-2" />

        <!-- Create new album -->
        <div class="px-4 py-3">
          <div class="text-body-2 text-medium-emphasis mb-2">
            Create new album under:
            <strong>{{
              selectedAlbumId
                ? (albumStore.albums.get(selectedAlbumId)?.displayName ?? '—')
                : '— (select a parent above)'
            }}</strong>
          </div>
          <div class="d-flex gap-2 align-center">
            <v-text-field
              v-model="newAlbumName"
              density="compact"
              variant="outlined"
              label="New album name"
              hide-details
              :disabled="!selectedAlbumId"
              @keyup.enter="handleCreate"
            />
            <v-btn
              icon="mdi-folder-plus"
              size="small"
              :disabled="!selectedAlbumId || !newAlbumName.trim()"
              :loading="creating"
              @click="handleCreate"
            />
          </div>
        </div>
      </v-card-text>

      <v-divider />

      <v-card-actions>
        <v-spacer />
        <v-btn variant="text" @click="cancel">Cancel</v-btn>
        <v-btn
          variant="tonal"
          color="primary"
          :disabled="!selectedAlbumId || selectedAlbumId === currentAlbumId"
          :loading="submitting"
          @click="handleSubmit"
          >Move</v-btn
        >
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useModalStore } from '@/store/modalStore'
import { useAlbumStore } from '@/store/albumStore'
import { useCollectionStore } from '@/store/collectionStore'
import { useDataStore } from '@/store/dataStore'
import { assignAlbum } from '@/api/assignAlbum'
import { createDirAlbum } from '@/api/createDirAlbum'
import { getHashIndexDataFromRoute, getIsolationIdByRoute } from '@utils/getter'
import type { AlbumInfo } from '@type/types'

const route = useRoute()
const isolationId = getIsolationIdByRoute(route)

const modalStore = useModalStore('mainId')
const albumStore = useAlbumStore('mainId')
const collectionStore = useCollectionStore(isolationId)
const dataStore = useDataStore(isolationId)

const search = ref('')
const selectedAlbumId = ref<string | null>(null)
const newAlbumName = ref('')
const submitting = ref(false)
const creating = ref(false)

// The album the item(s) currently belong to (for single-item mode, from route context)
const currentAlbumId = computed<string | null>(() => {
  if (modalStore.assignAlbumBatch) return null
  const parsed = getHashIndexDataFromRoute(route)
  if (!parsed) return null
  const { data } = parsed
  if (data.type !== 'image' && data.type !== 'video') return null
  return data.album
})

// ── Tree building ──────────────────────────────────────────────────────────────

interface FlatNode {
  id: string
  title: string
  dirPath: string | null
  depth: number
  visible: boolean
  matchesSelf: boolean
  hasVisibleDescendant: boolean
}

function buildFlatTree(albums: Map<string, AlbumInfo>, query: string): FlatNode[] {
  // Build parent→children index
  const children = new Map<string | null, string[]>()
  for (const [id, info] of albums) {
    const parent = info.parentAlbumId ?? null
    if (!children.has(parent)) children.set(parent, [])
    const bucket = children.get(parent)
    if (bucket !== undefined) bucket.push(id)
  }

  const q = query.toLowerCase().trim()

  // DFS traversal; collect nodes with depth
  const result: FlatNode[] = []

  function dfs(id: string, depth: number) {
    const info = albums.get(id)
    if (!info) return

    const title = info.displayName
    const matchesSelf =
      q === '' || title.toLowerCase().includes(q) || (info.dirPath ?? '').toLowerCase().includes(q)

    const nodeChildren = children.get(id) ?? []

    // Recurse first to determine if any descendant matches
    const childStartIdx = result.length
    // We'll push a placeholder and fill it after recursing children
    const placeholder: FlatNode = {
      id,
      title,
      dirPath: info.dirPath,
      depth,
      visible: false,
      matchesSelf,
      hasVisibleDescendant: false
    }
    result.push(placeholder)

    let childHasVisible = false
    for (const childId of nodeChildren) {
      const before = result.length
      dfs(childId, depth + 1)
      // Check if any child node became visible
      for (let i = before; i < result.length; i++) {
        if (result[i]?.visible === true) {
          childHasVisible = true
          break
        }
      }
    }

    placeholder.hasVisibleDescendant = childHasVisible
    placeholder.visible = matchesSelf || childHasVisible
    void childStartIdx // suppress unused warning
  }

  // Roots: albums without a parent that exists in the map
  const allIds = new Set(albums.keys())
  const roots = [...allIds].filter((id) => {
    const parent = albums.get(id)?.parentAlbumId ?? null
    return parent === null || !allIds.has(parent)
  })

  // Sort roots and children alphabetically
  roots.sort((a, b) =>
    (albums.get(a)?.displayName ?? '').localeCompare(albums.get(b)?.displayName ?? '')
  )

  for (const rootId of roots) {
    dfs(rootId, 0)
  }

  return result
}

const flatTree = computed<FlatNode[]>(() => {
  if (!albumStore.fetched) return []
  return buildFlatTree(albumStore.albums, search.value)
})

const visibleCount = computed(() => flatTree.value.filter((n) => n.visible).length)

// ── Lifecycle ─────────────────────────────────────────────────────────────────

onMounted(() => {
  if (!albumStore.fetched) {
    void albumStore.fetchAlbums()
  }
  // Pre-select the item's current album
  selectedAlbumId.value = currentAlbumId.value
})

// ── Actions ───────────────────────────────────────────────────────────────────

function cancel() {
  modalStore.showAssignAlbumModal = false
}

// True if `albumId` is nested (at any depth) under one of the other
// currently-selected albums. Moving an ancestor already carries its
// descendants along on disk — also moving a selected descendant would
// re-fetch its now-updated path and "extract" it back out as a sibling of
// the already-moved ancestor instead of leaving it nested, which is safe
// but not what selecting both together looks like it should do.
function isDescendantOfAnySelected(albumId: string, selectedAlbumIds: Set<string>): boolean {
  let parent = albumStore.albums.get(albumId)?.parentAlbumId ?? null
  while (parent !== null) {
    if (selectedAlbumIds.has(parent)) return true
    parent = albumStore.albums.get(parent)?.parentAlbumId ?? null
  }
  return false
}

async function handleSubmit() {
  if (selectedAlbumId.value === null) return
  submitting.value = true
  try {
    if (modalStore.assignAlbumBatch) {
      // Batch: move all selected items (images/videos move as a single
      // file; albums move as a whole directory — assignAlbum/assign_album
      // dispatch on the item's actual type either way).
      const indices = [...collectionStore.editModeCollection]
      const selectedAlbumIds = new Set<string>()
      for (const idx of indices) {
        const item = dataStore.data.get(idx)
        if (item?.type === 'album') selectedAlbumIds.add(item.id)
      }
      for (const idx of indices) {
        const item = dataStore.data.get(idx)
        if (!item) continue
        if (item.type === 'album' && isDescendantOfAnySelected(item.id, selectedAlbumIds)) {
          continue
        }
        await assignAlbum(item.id, selectedAlbumId.value, idx, isolationId)
      }
      collectionStore.leaveEdit()
    } else {
      // Single item from route context
      const parsed = getHashIndexDataFromRoute(route)
      if (!parsed) return
      const { hash, index } = parsed
      await assignAlbum(hash, selectedAlbumId.value, index, isolationId)
    }
  } finally {
    submitting.value = false
    modalStore.showAssignAlbumModal = false
  }
}

async function handleCreate() {
  if (selectedAlbumId.value === null || newAlbumName.value.trim() === '') return
  creating.value = true
  try {
    const newId = await createDirAlbum(selectedAlbumId.value, newAlbumName.value.trim())
    if (newId !== undefined) {
      selectedAlbumId.value = newId
      newAlbumName.value = ''
    }
  } finally {
    creating.value = false
  }
}
</script>

<style scoped>
.v-list-item.bg-primary-lighten-4 {
  background-color: rgba(var(--v-theme-primary), 0.12);
}
</style>
