<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">Shares</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-sheet v-if="shares.length > 0" border rounded="lg" class="mx-4 my-4 overflow-hidden">
        <template v-for="(item, i) in shares" :key="item.albumId + '-' + item.share.keyFingerprint">
          <v-divider v-if="i > 0"></v-divider>
          <v-list-item :title="item.share.description || item.share.keyFingerprint">
            <template #subtitle>
              <span>
                {{ item.displayName }}
                <span v-if="isExpired(item.share.exp)" class="text-error"> · expired</span>
                <span v-else-if="item.share.exp === 0"> · never expires</span>
                <span v-else> · expires {{ formatExpiry(item.share.exp) }}</span>
                <span v-if="item.share.hasPassword"> · password</span>
              </span>
            </template>
            <template #append>
              <v-tooltip v-if="!isCensored(item)" location="top" text="Edit">
                <template #activator="{ props }">
                  <v-btn
                    v-bind="props"
                    icon="mdi-pencil"
                    variant="text"
                    size="small"
                    aria-label="Edit share"
                    @click="openEdit(item)"
                  ></v-btn>
                </template>
              </v-tooltip>
              <v-tooltip v-if="!isCensored(item) && !isRevealed(item)" location="top" text="Reveal">
                <template #activator="{ props }">
                  <v-btn
                    v-bind="props"
                    icon="mdi-eye"
                    variant="text"
                    size="small"
                    aria-label="Reveal share link"
                    :loading="isRevealing(item)"
                    @click="reveal(item)"
                  ></v-btn>
                </template>
              </v-tooltip>
              <v-tooltip v-if="isRevealed(item)" location="top" text="Open">
                <template #activator="{ props }">
                  <v-btn
                    v-bind="props"
                    icon="mdi-open-in-new"
                    variant="text"
                    size="small"
                    aria-label="Open share"
                    :href="shareUrl(item)"
                    target="_blank"
                    tag="a"
                  ></v-btn>
                </template>
              </v-tooltip>
              <v-tooltip v-if="isRevealed(item) && showCopy" location="top" text="Copy">
                <template #activator="{ props }">
                  <v-btn
                    v-bind="props"
                    icon="mdi-content-copy"
                    variant="text"
                    size="small"
                    aria-label="Copy share link"
                    @click="copyLink(item)"
                  ></v-btn>
                </template>
              </v-tooltip>
              <v-tooltip location="top" text="Revoke">
                <template #activator="{ props }">
                  <v-btn
                    v-bind="props"
                    icon="mdi-delete-outline"
                    variant="text"
                    color="error"
                    size="small"
                    aria-label="Delete share"
                    @click="openDelete(item)"
                  ></v-btn>
                </template>
              </v-tooltip>
            </template>
          </v-list-item>
        </template>
      </v-sheet>

      <v-card-text v-else class="text-medium-emphasis">No shares yet</v-card-text>
    </v-card>

    <EditShareModal v-if="currentEdit !== null" :edit-share-data="currentEdit" />
    <ShareDeleteConfirmModal v-if="currentDelete !== null" :delete-share-data="currentDelete" />
  </v-col>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import axios from 'axios'
import { useAlbumStore } from '@/store/albumStore'
import { useModalStore } from '@/store/modalStore'
import { useMessageStore } from '@/store/messageStore'
import { ShareInfoSchema } from '@/type/schemas'
import type { EditShareData } from '@/type/types'
import { copyTextToClipboard, canShowCopyButton } from '@/script/utils/clipboard'
import EditShareModal from '@/components/Modal/EditShareModal.vue'
import ShareDeleteConfirmModal from '@/components/Modal/ShareDeleteConfirmModal.vue'

const albumStore = useAlbumStore('mainId')
const modalStore = useModalStore('mainId')
const messageStore = useMessageStore('mainId')

const currentEdit = ref<EditShareData | null>(null)
const currentDelete = ref<EditShareData | null>(null)

const revealed = ref<Map<string, string>>(new Map())
const revealing = ref<Set<string>>(new Set())
const censored = ref<Set<string>>(new Set())

/// Copy buttons render only where the secure Clipboard API can run.
const showCopy = computed(() => canShowCopyButton())

/// Flat list of every share across albums, mirroring the Links page table.
const shares = computed<EditShareData[]>(() => {
  const arr: EditShareData[] = []
  for (const album of albumStore.albums.values()) {
    for (const [, share] of album.shareList) {
      arr.push({
        albumId: album.albumId,
        displayName: album.displayName,
        share: ShareInfoSchema.parse(share)
      })
    }
  }
  return arr
})

const isExpired = (exp: number): boolean => exp !== 0 && exp * 1000 < Date.now()

const formatExpiry = (exp: number): string => new Date(exp * 1000).toLocaleString()

const isRevealed = (item: EditShareData): boolean => revealed.value.has(item.share.keyFingerprint)

const isRevealing = (item: EditShareData): boolean => revealing.value.has(item.share.keyFingerprint)

const isCensored = (item: EditShareData): boolean => censored.value.has(item.share.keyFingerprint)

const shareUrl = (item: EditShareData): string | undefined => {
  const token = revealed.value.get(item.share.keyFingerprint)
  if (token === undefined) return undefined
  return `${window.location.origin}/share/${item.albumId}-${token}`
}

const openEdit = (item: EditShareData) => {
  currentEdit.value = item
  modalStore.showEditShareModal = true
}

const openDelete = (item: EditShareData) => {
  currentDelete.value = item
  modalStore.showDeleteShareModal = true
}

const copyLink = async (item: EditShareData) => {
  const url = shareUrl(item)
  if (url === undefined) return
  const ok = await copyTextToClipboard(url)
  if (ok) {
    messageStore.success('Share URL copied to clipboard.')
  } else {
    messageStore.error('Copy failed — select the link manually')
  }
}

const reveal = async (item: EditShareData) => {
  const fp = item.share.keyFingerprint
  if (revealed.value.has(fp) || censored.value.has(fp)) return
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
    const nextCensored = new Set(censored.value)
    nextCensored.add(fp)
    censored.value = nextCensored
    messageStore.error('Share link not available')
  } finally {
    const done = new Set(revealing.value)
    done.delete(fp)
    revealing.value = done
  }
}

onMounted(async () => {
  if (!albumStore.fetched) await albumStore.fetchAlbums()
})
</script>
