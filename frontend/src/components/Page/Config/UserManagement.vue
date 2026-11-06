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

      <v-sheet v-if="userGroups.length > 0" border rounded="lg" class="mx-4 mb-4 overflow-hidden">
        <template v-for="(group, gi) in userGroups" :key="group.key">
          <v-divider v-if="gi > 0"></v-divider>
          <v-list-item v-for="user in group.users" :key="user.userId" :title="user.userId">
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
              <v-tooltip location="top" :text="shieldLabel(user)">
                <template #activator="{ props }">
                  <span v-bind="props" class="d-inline-flex">
                    <v-btn
                      :icon="user.admin ? 'mdi-shield-crown' : 'mdi-shield-outline'"
                      variant="text"
                      size="small"
                      :color="user.admin ? 'primary' : undefined"
                      :aria-label="shieldLabel(user)"
                      :aria-pressed="user.admin"
                      :disabled="isRowLocked(user) || saving"
                      @click="onToggle(user, !user.admin)"
                    ></v-btn>
                  </span>
                </template>
              </v-tooltip>
              <v-tooltip location="top" :text="lockLabel(user)">
                <template #activator="{ props }">
                  <span v-bind="props" class="d-inline-flex">
                    <v-btn
                      :icon="user.disabled ? 'mdi-lock' : 'mdi-lock-open-variant-outline'"
                      variant="text"
                      size="small"
                      :color="user.disabled ? 'warning' : undefined"
                      :aria-label="lockLabel(user)"
                      :aria-pressed="!!user.disabled"
                      :disabled="isRowLocked(user) || saving"
                      @click="onToggleDisabled(user, !user.disabled)"
                    ></v-btn>
                  </span>
                </template>
              </v-tooltip>
              <v-tooltip location="top" :text="shareLabel(user)">
                <template #activator="{ props }">
                  <span v-bind="props" class="d-inline-flex">
                    <v-btn
                      :icon="
                        user.sharesEnabled !== false ? 'mdi-share-variant' : 'mdi-share-off-variant'
                      "
                      variant="text"
                      size="small"
                      :color="user.sharesEnabled !== false ? undefined : 'warning'"
                      :aria-label="shareLabel(user)"
                      :aria-pressed="user.sharesEnabled !== false"
                      :disabled="isRowLocked(user) || saving"
                      @click="onToggleSharing(user, !(user.sharesEnabled !== false))"
                    ></v-btn>
                  </span>
                </template>
              </v-tooltip>
              <v-tooltip location="top" :text="deleteLabel(user)">
                <template #activator="{ props }">
                  <span v-bind="props" class="d-inline-flex">
                    <v-btn
                      icon="mdi-delete-outline"
                      variant="text"
                      color="error"
                      size="small"
                      :aria-label="deleteLabel(user)"
                      :disabled="isRowLocked(user) || saving"
                      @click="askDelete(user)"
                    ></v-btn>
                  </span>
                </template>
              </v-tooltip>
            </template>
          </v-list-item>
        </template>
      </v-sheet>

      <ConfirmDialog
        v-model="confirmOpen"
        :title="pendingAdmin ? 'Make admin?' : 'Demote admin?'"
        :confirm-label="pendingAdmin ? 'Promote' : 'Demote'"
        :loading="saving"
        @confirm="confirmRoleChange"
        @cancel="cancelRoleChange"
      >
        <span v-if="pendingAdmin">
          Grant admin rights to {{ pendingTarget?.userId }}? They will gain full access to this
          panel.
        </span>
        <span v-else-if="pendingTarget?.userId === me?.id">
          You are demoting your own account. You will lose access to this panel. Continue?
        </span>
        <span v-else> Remove admin rights from {{ pendingTarget?.userId }}? </span>
      </ConfirmDialog>

      <ConfirmDialog
        v-model="deleteConfirmOpen"
        title="Delete user?"
        confirm-label="Delete"
        cancel-label="Cancel"
        :loading="saving"
        destructive
        @confirm="confirmDelete"
        @cancel="cancelDelete"
      >
        Delete {{ pendingDelete?.userId }}? This cannot be undone.
        <v-text-field
          v-model="deletePassword"
          type="password"
          label="Your password"
          class="mt-2"
          @keyup.enter="confirmDelete"
        ></v-text-field>
      </ConfirmDialog>

      <ConfirmDialog
        v-model="setPasswordOpen"
        title="Set password"
        confirm-label="Set"
        cancel-label="Cancel"
        :loading="settingPassword"
        @confirm="confirmSetPassword"
        @cancel="cancelSetPassword"
      >
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
      </ConfirmDialog>
    </v-card>
  </v-col>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import ConfirmDialog from '@/components/Modal/ConfirmDialog.vue'
import axios from 'axios'
import {
  listUsers,
  setUserAdmin,
  setUserDisabled,
  setUserSharing,
  deleteUser,
  createUser,
  setUserPassword
} from '@/api/users'
import type { UserSummary } from '@/api/users'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { errorDisplay } from '@/script/utils/errorDisplay'
import { useCurrentUser, selfRowState, canSeeUserPanel } from '@/script/utils/currentUser'
import { groupUsersByRole } from '@/script/utils/userGroups'

const messageStore = useMessageStore('mainId')
const me = useCurrentUser()
const isAdmin = computed(() => canSeeUserPanel(me.value))

// --- State ---
const users = ref<UserSummary[]>([])
const saving = ref(false)
const confirmOpen = ref(false)
const pendingTarget = ref<UserSummary | null>(null)
const pendingAdmin = ref<boolean | null>(null)
const deleteConfirmOpen = ref(false)
const pendingDelete = ref<UserSummary | null>(null)
const deletePassword = ref('')
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
const userGroups = computed(() => groupUsersByRole(users.value))

const isRowLocked = (user: UserSummary): boolean =>
  selfRowState(me.value, user.userId, adminCount.value) === 'sole-admin'

// Tooltip and accessible name for the shield toggle. When demote is locked
// the phrasing carries the reason: the disabled button shows no subtitle
// and fires no hover of its own (the activator is the wrapper span).
const shieldLabel = (user: UserSummary): string => {
  if (isRowLocked(user)) return 'Cannot demote admin - promote another user first'
  return user.admin ? 'Demote admin' : 'Make admin'
}

// Same treatment for delete: the locked row keeps a visible but disabled
// trash with the reason instead of hiding the action. Deleting the sole
// admin is a backend 409, so the disabled state is truthful.
const deleteLabel = (user: UserSummary): string => {
  if (isRowLocked(user)) return 'Cannot delete admin - promote another user first'
  return 'Delete user'
}

const lockLabel = (user: UserSummary): string =>
  user.disabled === true ? 'Enable account' : 'Disable account'

const shareLabel = (user: UserSummary): string =>
  user.sharesEnabled !== false ? 'Disable sharing' : 'Enable sharing'

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

const applyDisabledChange = async (target: UserSummary, disabled: boolean) => {
  const previous = !disabled
  saving.value = true
  try {
    await setUserDisabled(target.userId, disabled)
    messageStore.success(disabled ? `${target.userId} disabled` : `${target.userId} enabled`)
    await refreshUsers()
  } catch (error: unknown) {
    target.disabled = previous
    if (!axios.isAxiosError(error) || error.response === undefined) {
      messageStore.error(errorDisplay(error))
    }
    await refreshUsers()
  } finally {
    saving.value = false
  }
}

const onToggleDisabled = (user: UserSummary, disabled: boolean) => {
  user.disabled = disabled
  void applyDisabledChange(user, disabled)
}

const applySharingChange = async (target: UserSummary, enabled: boolean) => {
  const previous = target.sharesEnabled !== false
  saving.value = true
  try {
    await setUserSharing(target.userId, enabled)
    messageStore.success(
      enabled ? `${target.userId} sharing enabled` : `${target.userId} sharing disabled`
    )
    await refreshUsers()
  } catch (error: unknown) {
    target.sharesEnabled = previous
    if (!axios.isAxiosError(error) || error.response === undefined) {
      messageStore.error(errorDisplay(error))
    }
    await refreshUsers()
  } finally {
    saving.value = false
  }
}

const onToggleSharing = (user: UserSummary, enabled: boolean) => {
  user.sharesEnabled = enabled
  void applySharingChange(user, enabled)
}

const onToggle = (user: UserSummary, admin: boolean) => {
  // Both directions stage the confirm dialog: granting admin hands over
  // full access, so it is never applied optimistically.
  pendingTarget.value = user
  pendingAdmin.value = admin
  confirmOpen.value = true
}

const cancelRoleChange = () => {
  confirmOpen.value = false
  pendingTarget.value = null
  pendingAdmin.value = null
}

const confirmRoleChange = async () => {
  const target = pendingTarget.value
  const admin = pendingAdmin.value
  confirmOpen.value = false
  pendingTarget.value = null
  pendingAdmin.value = null
  if (target === null || admin === null) return
  target.admin = admin
  await applyRoleChange(target, admin)
}

const askDelete = (user: UserSummary) => {
  pendingDelete.value = user
  deletePassword.value = ''
  deleteConfirmOpen.value = true
}

const cancelDelete = () => {
  deleteConfirmOpen.value = false
  pendingDelete.value = null
  deletePassword.value = ''
}

const confirmDelete = async () => {
  const target = pendingDelete.value
  deleteConfirmOpen.value = false
  pendingDelete.value = null
  if (target === null) return
  const password = deletePassword.value
  deletePassword.value = ''
  saving.value = true
  try {
    await deleteUser(target.userId, password)
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
