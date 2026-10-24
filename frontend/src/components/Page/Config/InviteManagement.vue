<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">Invites</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-card-text>
        <v-row dense>
          <v-col cols="12" sm="4">
            <v-text-field
              v-model="expiryInput"
              label="Expires in (hours)"
              placeholder="168"
              variant="outlined"
              density="comfortable"
              type="number"
              hide-details
            ></v-text-field>
          </v-col>
          <v-col cols="12" sm="5">
            <v-text-field
              v-model="usernameInput"
              label="Username (optional)"
              placeholder="Leave empty for open invite"
              variant="outlined"
              density="comfortable"
              hide-details
            ></v-text-field>
          </v-col>
          <v-col cols="12" sm="3" class="d-flex align-center">
            <v-btn color="primary" variant="flat" block :loading="creating" @click="create">
              Create Invite
            </v-btn>
          </v-col>
        </v-row>

        <v-alert
          v-if="createdLink"
          type="success"
          variant="tonal"
          class="mt-4"
          closable
          @click:close="createdLink = null"
        >
          <div class="text-body-2 mb-1">Invite link (shown once — copy it now):</div>
          <div class="d-flex align-center gap-2">
            <code class="text-caption">{{ createdLink }}</code>
            <v-btn size="small" variant="text" @click="copyLink">Copy</v-btn>
          </div>
        </v-alert>
      </v-card-text>

      <v-list>
        <v-list-item v-for="invite in invites" :key="invite.id" :title="invite.prefix + '…'">
          <template #subtitle>
            <span>
              by {{ invite.createdBy }}
              <span v-if="invite.username"> · for {{ invite.username }}</span>
              <span v-else> · open</span>
              <span v-if="invite.usedBy"> · used by {{ invite.usedBy }}</span>
              <span v-else-if="invite.expiresAt !== null">
                · expires {{ formatExpiry(invite.expiresAt) }}
              </span>
              <span v-else> · never expires</span>
            </span>
          </template>
          <template #append>
            <v-btn
              variant="text"
              color="error"
              size="small"
              :disabled="revoking === invite.id"
              @click="revoke(invite.id)"
            >
              Revoke
            </v-btn>
          </template>
        </v-list-item>
        <v-list-item v-if="invites.length === 0">
          <v-list-item-title class="text-medium-emphasis">No invites yet</v-list-item-title>
        </v-list-item>
      </v-list>
    </v-card>
  </v-col>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { listInvites, createInvite, revokeInvite } from '@/api/invites'
import type { InviteView } from '@/api/invites'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { errorDisplay } from '@/script/utils/errorDisplay'

const messageStore = useMessageStore('mainId')

const invites = ref<InviteView[]>([])
const expiryInput = ref('')
const usernameInput = ref('')
const creating = ref(false)
const revoking = ref<string | null>(null)
const createdLink = ref<string | null>(null)

const expiresInHours = computed(() => {
  const trimmed = expiryInput.value.trim()
  if (trimmed === '') return 168
  const parsed = Number(trimmed)
  return Number.isFinite(parsed) ? parsed : 168
})

const refreshInvites = async () => {
  await tryWithMessageStore('mainId', async () => {
    invites.value = await listInvites()
    return true
  })
}

const create = async () => {
  creating.value = true
  try {
    await tryWithMessageStore('mainId', async () => {
      const result = await createInvite(expiresInHours.value, usernameInput.value)
      createdLink.value = `${window.location.origin}/register?invite=${result.token}`
      messageStore.success('Invite created')
      usernameInput.value = ''
      expiryInput.value = ''
      await refreshInvites()
      return true
    })
  } finally {
    creating.value = false
  }
}

const revoke = async (id: string) => {
  revoking.value = id
  try {
    await tryWithMessageStore('mainId', async () => {
      await revokeInvite(id)
      messageStore.success('Invite revoked')
      await refreshInvites()
      return true
    })
  } catch (error: unknown) {
    // The global interceptor owns display for axios errors with a response.
    const axiosErr = error as { response?: unknown }
    if (axiosErr.response === undefined) {
      messageStore.error(errorDisplay(error))
    }
  } finally {
    revoking.value = null
  }
}

const copyLink = async () => {
  if (createdLink.value === null) return
  try {
    await navigator.clipboard.writeText(createdLink.value)
    messageStore.success('Link copied')
  } catch (error: unknown) {
    messageStore.error(errorDisplay(error))
  }
}

const formatExpiry = (expiresAt: number): string => {
  const date = new Date(expiresAt * 1000)
  return date.toLocaleString()
}

onMounted(refreshInvites)
</script>
