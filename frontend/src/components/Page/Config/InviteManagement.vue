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
              label="Expires in"
              placeholder="7d"
              :hint="expiryHint"
              persistent-hint
              variant="outlined"
              density="comfortable"
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
            <v-btn v-if="showCopy" size="small" variant="text" @click="copyLink">Copy</v-btn>
            <span v-else class="text-caption text-medium-emphasis">Select and copy manually</span>
          </div>
        </v-alert>
      </v-card-text>

      <v-sheet border rounded="lg" class="mx-4 mb-4 overflow-hidden">
        <v-list>
          <v-list-item v-for="invite in invites" :key="invite.id" :title="invite.prefix + '…'">
            <template #subtitle>
              <span>
                status: {{ status(invite) }} · by {{ invite.createdBy }}
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
      </v-sheet>
    </v-card>
  </v-col>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import {
  listInvites,
  createInvite,
  revokeInvite,
  buildInviteLink,
  getInviteLimits
} from '@/api/invites'
import type { InviteView } from '@/api/invites'
import { inviteStatus } from '@/script/utils/inviteStatus'
import { MAX_EXPIRY_HOURS, parseExpiryInput } from '@/script/utils/expiryInput'
import { copyTextToClipboard, canShowCopyButton } from '@/script/utils/clipboard'
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
const revokedIds = ref<Set<string>>(new Set())

/// Copy button renders only where the secure Clipboard API can run.
const showCopy = computed(() => canShowCopyButton())

// Bound the server enforces on `expiresInHours`, fetched when the panel
// builds; the mirrored const stands in until the fetch lands (and if it
// fails, the server's 400 is the backstop).
const maxExpiryHours = ref(MAX_EXPIRY_HOURS)

const expiryHint = computed(
  () => `Up to ${maxExpiryHours.value}h · units h/d/m · 0h = never expires`
)

const refreshInvites = async () => {
  await tryWithMessageStore('mainId', async () => {
    invites.value = await listInvites()
    return true
  })
}

const refreshLimits = async () => {
  await tryWithMessageStore('mainId', async () => {
    maxExpiryHours.value = (await getInviteLimits()).maxExpiryHours
    return true
  })
}

const create = async () => {
  const parsed = parseExpiryInput(expiryInput.value, maxExpiryHours.value)
  if (!parsed.ok) {
    messageStore.error(parsed.error)
    return
  }
  creating.value = true
  try {
    await tryWithMessageStore('mainId', async () => {
      const result = await createInvite(parsed.hours, usernameInput.value)
      createdLink.value = buildInviteLink(
        window.location.origin,
        result.token,
        result.invite.username ?? undefined
      )
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
      revokedIds.value.add(id)
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
  const ok = await copyTextToClipboard(createdLink.value)
  if (ok) {
    messageStore.success('Link copied')
  } else {
    messageStore.error('Copy failed — select the link manually')
  }
}

const formatExpiry = (expiresAt: number): string => {
  const date = new Date(expiresAt * 1000)
  return date.toLocaleString()
}

const status = (invite: InviteView): string =>
  inviteStatus(invite, Date.now() / 1000, revokedIds.value.has(invite.id))

onMounted(() => {
  void refreshInvites()
  void refreshLimits()
})
</script>
