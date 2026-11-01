<template>
  <PageTemplate
    preset="card"
    :ready="albumStore.fetched"
    width="wide"
    :fill-height="true"
    :card-class="['w-100', 'h-100', 'overflow-hidden']"
  >
    <template #overlay>
      <EditShareModal
        v-if="modalStore.showEditShareModal && currentEditShareData"
        :edit-share-data="currentEditShareData"
      />

      <ShareDeleteConfirmModal
        v-if="modalStore.showDeleteShareModal && currentDeleteShareData"
        :delete-share-data="currentDeleteShareData"
      />
    </template>

    <template #content>
      <v-data-table
        :headers="headers"
        :items="tableItems"
        :group-by="[{ key: 'albumId' }]"
        item-value="keyFingerprint"
        :items-per-page="-1"
        :sort-by="[{ key: 'share.keyFingerprint', order: 'asc' }]"
        class="h-100"
      >
        <!-- Description with tooltip -->
        <template #[`item.share.description`]="{ item }">
          <v-tooltip location="top" :open-on-click="true">
            <template #activator="{ props }">
              <span v-bind="props" class="text-truncate">
                {{ item.share.description }}
              </span>
            </template>
            <span>{{ item.share.description }}</span>
          </v-tooltip>
        </template>

        <!-- Access: password, download, upload, metadata as state icons -->
        <template #[`item.access`]="{ item }">
          <div class="d-flex flex-row justify-center ga-1">
            <v-tooltip
              location="top"
              :text="item.share.hasPassword ? 'Password required' : 'No password set'"
            >
              <template #activator="{ props }">
                <v-icon
                  v-bind="props"
                  icon="mdi-lock"
                  :color="item.share.hasPassword ? 'success' : 'grey'"
                  size="small"
                />
              </template>
            </v-tooltip>
            <v-tooltip
              location="top"
              :text="item.share.showDownload ? 'Download allowed' : 'Download disabled'"
            >
              <template #activator="{ props }">
                <v-icon
                  v-bind="props"
                  icon="mdi-download"
                  :color="item.share.showDownload ? 'success' : 'grey'"
                  size="small"
                />
              </template>
            </v-tooltip>
            <v-tooltip
              location="top"
              :text="item.share.showUpload ? 'Upload allowed' : 'Upload disabled'"
            >
              <template #activator="{ props }">
                <v-icon
                  v-bind="props"
                  icon="mdi-upload"
                  :color="item.share.showUpload ? 'success' : 'grey'"
                  size="small"
                />
              </template>
            </v-tooltip>
            <v-tooltip
              location="top"
              :text="item.share.showMetadata ? 'Metadata shown' : 'Metadata hidden'"
            >
              <template #activator="{ props }">
                <v-icon
                  v-bind="props"
                  icon="mdi-tag"
                  :color="item.share.showMetadata ? 'success' : 'grey'"
                  size="small"
                />
              </template>
            </v-tooltip>
          </div>
        </template>

        <!-- Expiration -->
        <template #[`item.share.exp`]="{ item }">
          <span v-if="item.share.exp === 0">Never</span>
          <span v-else-if="item.share.exp * 1000 < Date.now()" class="text-error">Expired</span>
          <span v-else>{{ formatExpiration(item.share.exp) }}</span>
        </template>

        <!-- Actions -->
        <template #[`item.actions`]="{ item }">
          <div class="d-flex flex-row justify-center ga-1">
            <v-tooltip location="top" text="Delete">
              <template #activator="{ props }">
                <v-btn
                  v-bind="props"
                  icon="mdi-delete"
                  variant="text"
                  size="small"
                  @click="openDeleteConfirm(item)"
                />
              </template>
            </v-tooltip>
            <v-tooltip location="top" text="Edit">
              <template #activator="{ props }">
                <v-btn
                  v-bind="props"
                  icon="mdi-pencil"
                  variant="text"
                  size="small"
                  @click="clickEditShare(item)"
                />
              </template>
            </v-tooltip>
            <v-tooltip location="top" text="Reveal">
              <template #activator="{ props }">
                <v-btn
                  v-bind="props"
                  icon="mdi-eye"
                  variant="text"
                  size="small"
                  :loading="isRevealing(item)"
                  @click="reveal(item)"
                  v-if="!isRevealed(item)"
                />
              </template>
            </v-tooltip>
            <v-tooltip location="top" text="Open" v-if="isRevealed(item)">
              <template #activator="{ props }">
                <v-btn
                  v-bind="props"
                  icon="mdi-open-in-new"
                  variant="text"
                  size="small"
                  :href="revealedUrl(item)"
                  target="_blank"
                  tag="a"
                />
              </template>
            </v-tooltip>
            <v-tooltip location="top" text="Copy" v-if="isRevealed(item)">
              <template #activator="{ props }">
                <v-btn
                  v-bind="props"
                  icon="mdi-content-copy"
                  variant="text"
                  size="small"
                  @click="performCopy(item)"
                />
              </template>
            </v-tooltip>
          </div>
        </template>

        <!-- Group header -->
        <template #group-header="{ item, columns, toggleGroup, isGroupOpen }">
          <tr>
            <td :colspan="columns.length">
              <div class="d-flex align-center">
                <v-btn
                  :icon="isGroupOpen(item) ? '$expand' : '$next'"
                  color="medium-emphasis"
                  density="comfortable"
                  size="small"
                  variant="outlined"
                  @click="toggleGroup(item)"
                />
                <span class="ms-4 font-weight-bold">
                  {{ albumStore.albums.get(item.value)?.displayName }}
                </span>
                <v-tooltip location="top" text="Open">
                  <template #activator="{ props }">
                    <v-btn
                      v-bind="props"
                      icon="mdi-open-in-new"
                      variant="text"
                      size="small"
                      class="ms-2"
                      :href="`${locationOrigin}/albums/view/${item.value}/read`"
                      target="_blank"
                      tag="a"
                    />
                  </template>
                </v-tooltip>
              </div>
            </td>
          </tr>
        </template>
      </v-data-table>
    </template>
  </PageTemplate>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, onMounted, onBeforeUnmount } from 'vue'
import { useClipboard } from '@vueuse/core'
import axios from 'axios'
import EditShareModal from '@/components/Modal/EditShareModal.vue'
import ShareDeleteConfirmModal from '@/components/Modal/ShareDeleteConfirmModal.vue'

import { useInitializedStore } from '@/store/initializedStore'
import { useAlbumStore } from '@/store/albumStore'
import { useSessionStore } from '@/store/sessionStore'
import { useModalStore } from '@/store/modalStore'
import { useMessageStore } from '@/store/messageStore'
import type { EditShareData } from '@/type/types'
import { ShareInfoSchema } from '@/type/schemas'
import PageTemplate from './PageLayout/PageTemplate.vue'

const initializedStore = useInitializedStore('mainId')
const albumStore = useAlbumStore('mainId')
const currentUserId = computed(() => useSessionStore('mainId').profile?.userId ?? null)
const modalStore = useModalStore('mainId')
const messageStore = useMessageStore('mainId')

const locationOrigin = window.location.origin
const { copy } = useClipboard()

const currentEditShareData = ref<EditShareData | null>(null)
const currentDeleteShareData = ref<EditShareData | null>(null)

const revealed = ref<Map<string, string>>(new Map())
const revealing = ref<Set<string>>(new Set())

const headers = [
  {
    title: 'Description',
    key: 'share.description',
    width: '200px',
    maxWidth: '200px',
    nowrap: true
  },
  { title: 'Expires', key: 'share.exp', nowrap: true },
  { title: 'Access', key: 'access', sortable: false },
  { title: 'Actions', key: 'actions', sortable: false }
]

const tableItems = computed<EditShareData[]>(() => {
  const arr: EditShareData[] = []
  for (const album of albumStore.albums.values()) {
    // Defensive: only manage shares on albums visible to the caller.
    if (album.owner != null && album.owner !== currentUserId.value) continue
    for (const [, share] of album.shareList) {
      const validatedShare = ShareInfoSchema.parse(share)
      arr.push({ albumId: album.albumId, displayName: album.displayName, share: validatedShare })
    }
  }
  return arr
})

function clickEditShare(data: EditShareData) {
  currentEditShareData.value = data
  modalStore.showEditShareModal = true
}

function openDeleteConfirm(data: EditShareData) {
  currentDeleteShareData.value = data
  modalStore.showDeleteShareModal = true
}

function isRevealed(item: EditShareData): boolean {
  return revealed.value.has(item.share.keyFingerprint)
}

function isRevealing(item: EditShareData): boolean {
  return revealing.value.has(item.share.keyFingerprint)
}

function revealedUrl(item: EditShareData): string | undefined {
  const token = revealed.value.get(item.share.keyFingerprint)
  if (token === undefined) return undefined
  return `${locationOrigin}/share/${item.albumId}-${token}`
}

async function reveal(item: EditShareData) {
  const fp = item.share.keyFingerprint
  if (revealed.value.has(fp)) return
  const nextRevealing = new Set(revealing.value)
  nextRevealing.add(fp)
  revealing.value = nextRevealing
  try {
    const result = await axios.get<{ token: string }>('/get/share/token', {
      params: { album_id: item.albumId, fingerprint: fp }
    })
    const nextRevealed = new Map(revealed.value)
    nextRevealed.set(fp, result.data.token)
    revealed.value = nextRevealed
  } catch {
    messageStore.error('Share link not available')
  } finally {
    const done = new Set(revealing.value)
    done.delete(fp)
    revealing.value = done
  }
}

async function performCopy(item: EditShareData) {
  const url = revealedUrl(item)
  if (url === undefined) return
  await copy(url)
  messageStore.success('Share URL copied to clipboard.')
}

function formatExpiration(exp: number): string {
  const date = new Date(exp * 1000)
  return date.toLocaleString()
}

onMounted(async () => {
  if (!albumStore.fetched) await albumStore.fetchAlbums()
  initializedStore.initialized = true
  await nextTick()

  // auto-expand all groups
  const groupButtons = Array.from(document.querySelectorAll('button.v-btn')).filter((btn) =>
    btn.querySelector('.mdi-chevron-right')
  ) as HTMLButtonElement[]
  groupButtons.forEach((btn) => {
    btn.click()
  })
})

onBeforeUnmount(() => {
  initializedStore.initialized = false
})
</script>
