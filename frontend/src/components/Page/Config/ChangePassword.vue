<template>
  <v-col cols="12">
    <SectionCard title="Change Password">
      <v-list-item>
        <v-text-field
          v-model="oldPassword"
          class="pt-2"
          prepend-icon="mdi-lock-outline"
          label="Current Password"
          :type="showOldPassword ? 'text' : 'password'"
          :append-inner-icon="showOldPassword ? 'mdi-eye' : 'mdi-eye-off'"
          variant="outlined"
          :rules="[rules.required]"
          persistent-placeholder
          density="compact"
          @click:append-inner="showOldPassword = !showOldPassword"
        ></v-text-field>
      </v-list-item>
      <v-list-item>
        <v-text-field
          v-model="newPassword"
          class="pt-2"
          prepend-icon="mdi-lock"
          label="New Password"
          :type="showNewPassword ? 'text' : 'password'"
          :append-inner-icon="showNewPassword ? 'mdi-eye' : 'mdi-eye-off'"
          variant="outlined"
          :rules="[rules.required]"
          persistent-placeholder
          density="compact"
          @click:append-inner="showNewPassword = !showNewPassword"
        ></v-text-field
      ></v-list-item>
      <v-list-item>
        <v-text-field
          v-model="confirmPassword"
          class="pt-2"
          prepend-icon="mdi-lock-check"
          label="Confirm New Password"
          :type="showConfirmPassword ? 'text' : 'password'"
          :append-inner-icon="showConfirmPassword ? 'mdi-eye' : 'mdi-eye-off'"
          variant="outlined"
          :rules="[rules.required, rules.matchesNew]"
          persistent-placeholder
          density="compact"
          @click:append-inner="showConfirmPassword = !showConfirmPassword"
        ></v-text-field
      ></v-list-item>
      <template #actions>
        <v-btn
          color="primary"
          variant="flat"
          :loading="loading"
          :disabled="!canSubmit"
          class="text-none"
          @click="savePassword"
        >
          Change Password
        </v-btn>
      </template>
    </SectionCard>
  </v-col>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import SectionCard from '@/components/Page/Config/SectionCard.vue'
import { setUserPassword } from '@/api/users'
import { useCurrentUser } from '@/script/utils/currentUser'
import { useMessageStore } from '@/store/messageStore'
import { useModalStore } from '@/store/modalStore'
import { useRedirectionStore } from '@/store/redirectionStore'
import { useSessionStore } from '@/store/sessionStore'
import { useConstStore } from '@/store/constStore'
import { applyVuetifyTheme } from '@/script/utils/vuetifyTheme'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { isPasswordChangeSubmittable, submitPasswordChange } from '@utils/passwordChange'

const messageStore = useMessageStore('mainId')
const currentUser = useCurrentUser()

// --- State ---
const oldPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const loading = ref(false)

const showOldPassword = ref(false)
const showNewPassword = ref(false)
const showConfirmPassword = ref(false)

// Submit is enabled only when the current password is non-empty and the
// new password is non-empty and confirmed.
const canSubmit = computed(() =>
  isPasswordChangeSubmittable(oldPassword.value, newPassword.value, confirmPassword.value)
)

// --- Validation Rules ---
const rules = {
  required: (v: string) => !!v || 'This field is required',
  matchesNew: (v: string) => v === newPassword.value || 'Passwords do not match'
}

// --- Actions ---
const savePassword = async () => {
  if (!canSubmit.value) return
  const userId = currentUser.value?.id
  if (userId === undefined) return

  loading.value = true

  await tryWithMessageStore('mainId', async () => {
    const ok = await submitPasswordChange(
      oldPassword.value,
      newPassword.value,
      confirmPassword.value,
      (old, next) => setUserPassword(userId, next, old)
    )
    if (!ok) return false

    // The store write bumps the user's token version: every outstanding
    // token dies with the old password, so this session is over. Mirror
    // the interceptor's session-death path — drop the profile and land on
    // login — instead of leaving the settings modal open on red (cleared)
    // fields. The success toast confirms what happened; the login page
    // banners on arrival.
    messageStore.success('Password updated successfully')
    useModalStore('mainId').showUserSettingsModal = false
    useSessionStore('mainId').clearProfile()
    const constStore = useConstStore('mainId')
    await constStore.loadThemeFor(null)
    applyVuetifyTheme(constStore.theme)
    await useRedirectionStore('mainId').redirectionToLogin()

    // Reset local form state
    oldPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
    return true
  })

  loading.value = false
}
</script>
