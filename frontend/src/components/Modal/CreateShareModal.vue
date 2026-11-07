<template>
  <ShareModalBase
    id="create-share-modal"
    v-model="modalStore.showShareModal"
    v-model:form-state="formState"
    title="Share Settings"
    mode="create"
    :share-link="shareLink"
    :loading="loading"
    @create="createLink"
    @update="updateLink"
  />
</template>

<script setup lang="ts">
import ShareModalBase from '@/components/Modal/ShareModalBase.vue'
import { ShareFormData } from '@type/types'
import { useModalStore } from '@/store/modalStore'

import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import axios from 'axios'
import { ref, Ref, watch } from 'vue'

const props = defineProps<{
  albumId: string
}>()

const modalStore = useModalStore('mainId')
const messageStore = useMessageStore('mainId')

const defaultFormState: ShareFormData = {
  description: '',
  passwordRequired: false,
  password: '',
  passwordSet: false,
  passwordDirty: false,
  expireEnabled: false,
  expDuration: null,
  showUpload: false,
  showDownload: true,
  showMetadata: false
}

const formState = ref<ShareFormData>({ ...defaultFormState })

watch(
  () => modalStore.showShareModal,
  (val) => {
    if (!val) {
      formState.value = { ...defaultFormState }
      createdShareKey.value = null
      createdFingerprint.value = null
      shareLink.value = null
    }
  }
)

const shareLink: Ref<string | null> = ref(null)
const createdShareKey: Ref<string | null> = ref(null)
const createdFingerprint: Ref<string | null> = ref(null)
const loading = ref(false)

const createLink = async (formData: ShareFormData) => {
  loading.value = true
  const expirationTimestamp =
    formData.expireEnabled && formData.expDuration !== null
      ? Math.floor(Date.now() / 1000) + formData.expDuration * 60
      : 0

  try {
    await tryWithMessageStore('mainId', async () => {
      const result = await axios.post<{ link: string; fingerprint: string }>('/post/create_share', {
        albumId: props.albumId,
        description: formData.description,
        password: formData.passwordRequired ? formData.password : null,
        showMetadata: formData.showMetadata,
        showDownload: formData.showDownload,
        showUpload: formData.showUpload,
        exp: expirationTimestamp
      })

      createdShareKey.value = result.data.link
      createdFingerprint.value = result.data.fingerprint
      shareLink.value = `${window.location.origin}/share/${props.albumId}-${result.data.link}`
      messageStore.success('Share link created successfully.')
    })
  } catch (e) {
    console.error('Create failed', e)
  } finally {
    loading.value = false
  }
}

const updateLink = async (formData: ShareFormData) => {
  if (createdFingerprint.value === null || createdFingerprint.value === '') return

  loading.value = true
  const expirationTimestamp =
    formData.expireEnabled && formData.expDuration !== null
      ? Math.floor(Date.now() / 1000) + formData.expDuration * 60
      : 0

  try {
    await tryWithMessageStore('mainId', async () => {
      await axios.put('/put/edit_share', {
        albumId: props.albumId,
        share: {
          url: createdFingerprint.value,
          description: formData.description,
          password: formData.passwordRequired ? formData.password : null,
          showMetadata: formData.showMetadata,
          showDownload: formData.showDownload,
          showUpload: formData.showUpload,
          exp: expirationTimestamp
        }
      })
      messageStore.success('Share settings updated.')
    })
  } catch (e) {
    console.error('Update failed', e)
  } finally {
    loading.value = false
  }
}
</script>
