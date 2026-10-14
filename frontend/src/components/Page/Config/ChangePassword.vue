<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">Change Password</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-list-item>
        <v-text-field
          class="pt-2"
          v-model="oldPassword"
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
          class="pt-2"
          prepend-icon="mdi-lock"
          v-model="newPassword"
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
          class="pt-2"
          prepend-icon="mdi-lock-check"
          v-model="confirmPassword"
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
      <v-card-actions class="justify-end px-4 pb-4">
        <v-btn
          color="primary"
          variant="flat"
          :loading="loading"
          :disabled="!canSubmit"
          @click="savePassword"
          class="text-none"
        >
          Save Changes
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-col>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { updatePassword } from '@/api/config'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { isPasswordChangeSubmittable, submitPasswordChange } from '@utils/passwordChange'

const messageStore = useMessageStore('mainId')

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

  loading.value = true

  await tryWithMessageStore('mainId', async () => {
    const ok = await submitPasswordChange(
      oldPassword.value,
      newPassword.value,
      confirmPassword.value,
      updatePassword
    )
    if (!ok) return false

    messageStore.success('Password updated successfully')

    // Reset local form state
    oldPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
    return true
  })

  loading.value = false
}
</script>
