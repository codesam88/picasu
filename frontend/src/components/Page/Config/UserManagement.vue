<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">User Management</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-list-item v-for="user in users" :key="user.userId" :title="user.userId">
        <template #append>
          <v-btn
            v-if="!isRowLocked(user)"
            icon="mdi-delete-outline"
            variant="text"
            color="error"
            size="small"
            :disabled="saving"
            @click="askDelete(user)"
          ></v-btn>
          <v-switch
            v-model="user.admin"
            color="primary"
            hide-details
            inset
            :disabled="isRowLocked(user) || saving"
            @change="onToggle(user, $event)"
          ></v-switch>
        </template>
        <template #subtitle>
          <span v-if="isRowLocked(user)"
            >Promote someone else first to demote your own account</span
          >
          <span v-else-if="user.admin">Admin</span>
          <span v-else>Standard user</span>
        </template>
      </v-list-item>

      <v-dialog v-model="confirmOpen" max-width="480">
        <v-card>
          <v-card-title class="font-weight-bold">Demote admin?</v-card-title>
          <v-card-text>
            <span v-if="pendingTarget?.userId === me?.id">
              You are demoting your own account. You will lose access to this panel. Continue?
            </span>
            <span v-else> Remove admin rights from {{ pendingTarget?.userId }}? </span>
          </v-card-text>
          <v-card-actions class="justify-end">
            <v-btn variant="text" @click="cancelDemote">Cancel</v-btn>
            <v-btn color="primary" variant="flat" :loading="saving" @click="confirmDemote">
              Demote
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-dialog>

      <v-dialog v-model="deleteConfirmOpen" max-width="480">
        <v-card>
          <v-card-title class="font-weight-bold">Delete user?</v-card-title>
          <v-card-text> Delete {{ pendingDelete?.userId }}? This cannot be undone. </v-card-text>
          <v-card-actions class="justify-end">
            <v-btn variant="text" @click="cancelDelete">Cancel</v-btn>
            <v-btn color="error" variant="flat" :loading="saving" @click="confirmDelete">
              Delete
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-dialog>
    </v-card>
  </v-col>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import axios from 'axios'
import { listUsers, setUserAdmin, deleteUser } from '@/api/users'
import type { UserSummary } from '@/api/users'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { errorDisplay } from '@/script/utils/errorDisplay'
import { useCurrentUser, selfRowState } from '@/script/utils/currentUser'

const messageStore = useMessageStore('mainId')
const me = useCurrentUser()

// --- State ---
const users = ref<UserSummary[]>([])
const saving = ref(false)
const confirmOpen = ref(false)
const pendingTarget = ref<UserSummary | null>(null)
const deleteConfirmOpen = ref(false)
const pendingDelete = ref<UserSummary | null>(null)

// --- Computed ---
const adminCount = computed(() => users.value.filter((u) => u.admin).length)

const isRowLocked = (user: UserSummary): boolean =>
  selfRowState(me.value, user.userId, adminCount.value) === 'sole-admin'

// --- Actions ---
const refreshUsers = async () => {
  await tryWithMessageStore('mainId', async () => {
    users.value = await listUsers()
    return true
  })
}

const applyRoleChange = async (target: UserSummary, admin: boolean) => {
  const previous = !admin
  saving.value = true
  try {
    await setUserAdmin(target.userId, admin)
    messageStore.success(
      admin ? `${target.userId} is now an admin` : `${target.userId} is no longer an admin`
    )
    await refreshUsers()
  } catch (error: unknown) {
    // Optimistic toggle failed: revert the switch and reload server state.
    // The global axios interceptor already displays verbatim server messages
    // and redirects on 401; only fill the gaps it leaves (no-response errors).
    target.admin = previous
    if (!axios.isAxiosError(error) || error.response === undefined) {
      messageStore.error(errorDisplay(error))
    }
    await refreshUsers()
  } finally {
    saving.value = false
  }
}

const onToggle = (user: UserSummary, value: unknown) => {
  const admin = value === true
  if (!admin) {
    // Demotion always requires confirmation; revert until confirmed.
    user.admin = true
    pendingTarget.value = user
    confirmOpen.value = true
    return
  }
  void applyRoleChange(user, true)
}

const cancelDemote = () => {
  confirmOpen.value = false
  pendingTarget.value = null
}

const confirmDemote = async () => {
  const target = pendingTarget.value
  confirmOpen.value = false
  pendingTarget.value = null
  if (target === null) return
  target.admin = false
  await applyRoleChange(target, false)
}

const askDelete = (user: UserSummary) => {
  pendingDelete.value = user
  deleteConfirmOpen.value = true
}

const cancelDelete = () => {
  deleteConfirmOpen.value = false
  pendingDelete.value = null
}

const confirmDelete = async () => {
  const target = pendingDelete.value
  deleteConfirmOpen.value = false
  pendingDelete.value = null
  if (target === null) return
  saving.value = true
  try {
    await deleteUser(target.userId)
    messageStore.success(`${target.userId} deleted`)
    await refreshUsers()
  } catch (error: unknown) {
    if (!axios.isAxiosError(error) || error.response === undefined) {
      messageStore.error(errorDisplay(error))
    }
    await refreshUsers()
  } finally {
    saving.value = false
  }
}

onMounted(refreshUsers)
</script>
