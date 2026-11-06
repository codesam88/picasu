<template>
  <ConfirmDialog
    v-if="submit !== undefined"
    v-model="modalStore.showDeleteShareModal"
    title="Delete&nbsp;Link"
    :width="400"
    confirm-label="OK"
    cancel-label="Cancel"
    destructive
    hide-close
    @confirm="submit"
    @cancel="modalStore.showDeleteShareModal = false"
  >
    <p class="mb-2">Are you sure you want to delete this share link?</p>
    <p class="text-medium-emphasis text-truncate">
      {{ shareUrl }}
    </p>
  </ConfirmDialog>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import axios from 'axios'
import ConfirmDialog from '@/components/Modal/ConfirmDialog.vue'
import { useModalStore } from '@/store/modalStore'
import { useAlbumStore } from '@/store/albumStore'
import { useMessageStore } from '@/store/messageStore'
import type { EditShareData } from '@/type/types'
import { tryWithMessageStore } from '@/script/utils/try_catch'

const props = defineProps<{ deleteShareData: EditShareData }>()

const modalStore = useModalStore('mainId')
const albumStore = useAlbumStore('mainId')
const messageStore = useMessageStore('mainId')

const shareUrl = props.deleteShareData.share.keyFingerprint

const submit = ref<(() => void) | undefined>()

onMounted(() => {
  submit.value = () => {
    void tryWithMessageStore('mainId', async () => {
      await axios.put('/put/delete_share', {
        albumId: props.deleteShareData.albumId,
        shareId: props.deleteShareData.share.keyFingerprint
      })

      messageStore.success('Share deleted')
      modalStore.showDeleteShareModal = false
      await albumStore.fetchAlbums()
    })
  }
})
</script>
