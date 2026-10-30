<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">User Management</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-card-text v-if="isAdmin">
        <v-row dense>
          <v-col cols="12" sm="5">
            <v-text-field
              v-model="newUsername"
              label="Username"
              variant="outlined"
              density="comfortable"
              hide-details
            ></v-text-field>
          </v-col>
          <v-col cols="12" sm="5">
            <v-text-field
              v-model="newPassword"
              label="Initial password"
              type="password"
              variant="outlined"
              density="comfortable"
              hide-details
            ></v-text-field>
          </v-col>
          <v-col cols="12" sm="2" class="d-flex align-center">
            <v-btn
              color="primary"
              variant="flat"
              block
              :loading="creating"
              @click="createUserAction"
            >
              Create
            </v-btn>
          </v-col>
        </v-row>
      </v-card-text>

      <v-list-item v-for="user in users" :key="user.userId" :title="user.userId">
        <template #append>
          <v-tooltip v-if="isAdmin" location="top" text="Set password">
            <template #activator="{ props }">
              <v-btn
                v-bind="props"
                icon="mdi-key-variant"
                variant="text"
                color="primary"
                size="small"
                aria-label="Set password"
                :disabled="saving"
                @click="askSetPassword(user)"
              ></v-btn>
            </template>
          </v-tooltip>
          <v-tooltip location="top" :text="user.admin ? 'Demote admin' : 'Make admin'">
            <template #activator="{ props }">
              <v-btn
                v-bind="props"
                :icon="user.admin ? 'mdi-shield-crown' : 'mdi-shield-outline'"
                variant="text"
                size="small"
                :color="user.admin ? 'primary' : undefined"
                :aria-label="user.admin ? 'Demote admin' : 'Make admin'"
                :aria-pressed="user.admin"
                :disabled="isRowLocked(user) || saving"
                @click="onToggle(user, !user.admin)"
              ></v-btn>
            </template>
          </v-tooltip>
          <v-tooltip v-if="!isRowLocked(user)" location="top" text="Delete user">
            <template #activator="{ props }">
              <v-btn
                v-bind="props"
                icon="mdi-delete-outline"
                variant="text"
                color="error"
                size="small"
                aria-label="Delete user"
                :disabled="saving"
                @click="askDelete(user)"
              ></v-btn>
            </template>
          </v-tooltip>
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

      <v-dialog v-model="setPasswordOpen" max-width="480">
        <v-card>
          <v-card-title class="font-weight-bold">Set password</v-card-title>
          <v-card-text>
            Set a new password for {{ pendingSetPassword?.userId }}. Enter your admin password to
            confirm.
            <v-text-field
              v-model="newSetPassword"
              type="password"
              label="New password"
              class="mt-2"
              @keyup.enter="confirmSetPassword"
            ></v-text-field>
            <v-text-field
              v-model="adminPassword"
              type="password"
              label="Your password"
              class="mt-2"
              @keyup.enter="confirmSetPassword"
            ></v-text-field>
          </v-card-text>
          <v-card-actions class="justify-end">
            <v-btn variant="text" @click="cancelSetPassword">Cancel</v-btn>
            <v-btn
              color="primary"
              variant="flat"
              :loading="settingPassword"
              @click="confirmSetPassword"
            >
              Set
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
import { listUsers, setUserAdmin, deleteUser, createUser, setUserPassword } from '@/api/users'
import type { UserSummary } from '@/api/users'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { errorDisplay } from '@/script/utils/errorDisplay'
import { useCurrentUser, selfRowState, canSeeUserPanel } from '@/script/utils/currentUser'

const messageStore = useMessageStore('mainId')
const me = useCurrentUser()
const isAdmin = computed(() => canSeeUserPanel(me.value))

// --- State ---
const users = ref<UserSummary[]>([])
const saving = ref(false)
const confirmOpen = ref(false)
const pendingTarget = ref<UserSummary | null>(null)
const deleteConfirmOpen = ref(false)
const pendingDelete = ref<UserSummary | null>(null)
const newUsername = ref('')
const newPassword = ref('')
const creating = ref(false)
const setPasswordOpen = ref(false)
const pendingSetPassword = ref<UserSummary | null>(null)
const newSetPassword = ref('')
const adminPassword = ref('')
const settingPassword = ref(false)

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

const onToggle = (user: UserSummary, admin: boolean) => {
  if (!admin) {
    // Demotion always requires confirmation. The shield button passes the
    // flipped flag explicitly, so nothing has mutated yet: just stage the
    // confirm.
    pendingTarget.value = user
    confirmOpen.value = true
    return
  }
  // Optimistic flip; applyRoleChange reverts to false and reloads on failure.
  user.admin = true
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

const createUserAction = async () => {
  if (newUsername.value.trim() === '' || newPassword.value === '') return
  creating.value = true
  try {
    await tryWithMessageStore('mainId', async () => {
      await createUser(newUsername.value.trim(), newPassword.value)
      messageStore.success(`${newUsername.value.trim()} created`)
      newUsername.value = ''
      newPassword.value = ''
      await refreshUsers()
      return true
    })
  } catch (error: unknown) {
    const axiosErr = error as { response?: unknown }
    if (axiosErr.response === undefined) {
      messageStore.error(errorDisplay(error))
    }
  } finally {
    creating.value = false
  }
}

const askSetPassword = (user: UserSummary) => {
  pendingSetPassword.value = user
  newSetPassword.value = ''
  adminPassword.value = ''
  setPasswordOpen.value = true
}

const cancelSetPassword = () => {
  setPasswordOpen.value = false
  pendingSetPassword.value = null
  newSetPassword.value = ''
  adminPassword.value = ''
}

const confirmSetPassword = async () => {
  const target = pendingSetPassword.value
  if (target === null) return
  setPasswordOpen.value = false
  settingPassword.value = true
  const newPasswordValue = newSetPassword.value
  const adminPwd = adminPassword.value
  try {
    await tryWithMessageStore('mainId', async () => {
      await setUserPassword(target.userId, newPasswordValue, adminPwd)
      messageStore.success(`Password set for ${target.userId}`)
      newSetPassword.value = ''
      adminPassword.value = ''
      await refreshUsers()
      return true
    })
  } catch (error: unknown) {
    const axiosErr = error as { response?: unknown }
    if (axiosErr.response === undefined) {
      messageStore.error(errorDisplay(error))
    }
  } finally {
    settingPassword.value = false
    pendingSetPassword.value = null
  }
}

onMounted(refreshUsers)
</script>
