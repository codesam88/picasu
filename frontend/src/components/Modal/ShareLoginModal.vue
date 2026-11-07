<template>
  <BaseModal
    v-model="modalStore.showShareLoginModal"
    :title="shareStore.isLinkExpired ? 'Link Expired' : 'Password Required'"
    :width="400"
    hide-close
  >
    <div v-if="shareStore.isLinkExpired">
      This share link has expired and is no longer accessible.
    </div>

    <div v-else>
      <div>This share link is protected. Please enter the password to continue.</div>
      <v-text-field
        v-model="password"
        label="Password"
        type="password"
        variant="outlined"
        :error-messages="errorMessage"
        autofocus
        :loading="loading"
        :disabled="loading"
        class="mt-3"
        @keyup.enter="submit"
      ></v-text-field>
    </div>

    <template v-if="!shareStore.isLinkExpired" #actions>
      <v-spacer />
      <v-btn color="primary" variant="elevated" :loading="loading" @click="submit"> Unlock </v-btn>
    </template>
  </BaseModal>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import BaseModal from '@/components/Modal/BaseModal.vue'
import { useModalStore } from '@/store/modalStore'
import { useShareStore } from '@/store/shareStore'
import axios from 'axios'

const modalStore = useModalStore('mainId')

const shareStore = useShareStore('mainId')

const password = ref('')

const errorMessage = ref('')
const loading = ref(false)

watch(
  () => modalStore.showShareLoginModal,
  (val) => {
    if (val) {
      errorMessage.value = ''
      password.value = ''
      loading.value = false
    }
  }
)

const submit = async () => {
  if (!password.value) return

  loading.value = true
  errorMessage.value = ''

  try {
    await axios.post(
      '/get/prefetch',
      {},
      {
        params: {
          locate: shareStore.albumId
        },
        headers: {
          'x-album-id': shareStore.albumId,
          'x-share-id': shareStore.shareId,
          'x-share-password': password.value
        }
      }
    )

    shareStore.password = password.value
    errorMessage.value = ''
    password.value = ''
    modalStore.showShareLoginModal = false
  } catch (error: unknown) {
    if (axios.isAxiosError(error)) {
      const status = error.response?.status

      if (status === 403) {
        // The global interceptor already toasted the share-403 message;
        // mark expiry locally without a second snackbar.
        shareStore.isLinkExpired = true
      } else if (status === 401) {
        errorMessage.value = 'Incorrect password'
      } else {
        errorMessage.value = 'Server error or invalid request.'
      }
    } else {
      errorMessage.value = 'An unexpected error occurred.'
    }
  } finally {
    loading.value = false
  }
}
</script>
