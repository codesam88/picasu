<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">User Management</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-list-item v-for="user in users" :key="user.userId" :title="user.userId">
        <template #append>
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
    </v-card>
  </v-col>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import axios from 'axios'
import { listUsers, setUserAdmin } from '@/api/users'
import type { UserSummary } from '@/api/users'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { errorDisplay } from '@/script/utils/errorDisplay'
import { useCurrentUser, selfRowState, errorToMessage } from '@/script/utils/currentUser'

const messageStore = useMessageStore('mainId')
const router = useRouter()
const me = useCurrentUser()

// --- State ---
const users = ref<UserSummary[]>([])
const saving = ref(false)
const confirmOpen = ref(false)
const pendingTarget = ref<UserSummary | null>(null)

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
    // and redirects on 401; only fill the gaps it leaves (no-response errors)
    // and make the login redirect explicit.
    target.admin = previous
    const status = axios.isAxiosError(error) ? error.response?.status : undefined
    if (!axios.isAxiosError(error) || error.response === undefined) {
      messageStore.error(errorDisplay(error))
    }
    const action = errorToMessage(status)
    if (action.redirectLogin && router.currentRoute.value.name !== 'login') {
      await router.push({ name: 'login' })
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

onMounted(refreshUsers)
</script>
