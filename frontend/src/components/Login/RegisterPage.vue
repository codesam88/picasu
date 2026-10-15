<template>
  <v-container fluid class="fill-height">
    <!-- Theme toggle button positioned at top right -->
    <v-tooltip location="top" text="Theme">
      <template #activator="{ props }">
        <v-btn
          v-bind="props"
          class="theme-toggle-btn"
          :icon="themeIsLight ? 'mdi-weather-sunny' : 'mdi-weather-night'"
          @click="themeIsLight = !themeIsLight"
          size="large"
          variant="text"
        />
      </template>
    </v-tooltip>

    <v-row class="fill-height justify-center align-center">
      <v-col cols="12" sm="8" md="6" lg="4">
        <v-card class="register-card mx-auto" elevation="8" rounded="lg">
          <v-card-text class="pa-8">
            <div class="text-center mb-6">
              <h2 class="text-h4 font-weight-light mb-2">Create Admin Account</h2>
              <p class="text-body-2">No administrator is enrolled yet. Register the first one.</p>
            </div>

            <v-form @submit.prevent="handleRegister" ref="form">
              <v-text-field
                v-model="userId"
                label="User ID"
                placeholder="User ID"
                variant="outlined"
                density="comfortable"
                required
                class="mb-4"
                :rules="[rules.required]"
              ></v-text-field>

              <v-text-field
                v-model="password"
                :type="showPassword ? 'text' : 'password'"
                label="Password"
                placeholder="Password"
                variant="outlined"
                density="comfortable"
                :append-inner-icon="showPassword ? 'mdi-eye' : 'mdi-eye-off'"
                @click:append-inner="showPassword = !showPassword"
                required
                class="mb-4"
                :rules="[rules.required]"
              ></v-text-field>

              <v-text-field
                v-model="confirmPassword"
                :type="showPassword ? 'text' : 'password'"
                label="Confirm Password"
                placeholder="Confirm Password"
                variant="outlined"
                density="comfortable"
                required
                class="mb-4"
                :rules="[rules.required, rules.matches]"
              ></v-text-field>

              <v-btn type="submit" color="primary" size="large" block :loading="loading">
                Register
              </v-btn>
            </v-form>
          </v-card-text>
        </v-card>
      </v-col>
    </v-row>
  </v-container>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useMessageStore } from '@/store/messageStore'
import { useSessionStore } from '@/store/sessionStore'
import { errorDisplay } from '@/script/utils/errorDisplay'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { useConstStore } from '@/store/constStore'
import { useTheme } from 'vuetify'
import { bootstrapUser, login } from '@/api/users'
import { submitRegistration } from '@/script/utils/registration'
import {
  resetEnrollmentProbe,
  runEnrollmentBoot,
  getEnrollmentProbe
} from '@/script/utils/enrollment'

const password = ref('')
const confirmPassword = ref('')
const showPassword = ref(false)
const loading = ref(false)

// Prefill from the last successful login; empty on first run.
const LAST_USER_ID_KEY = 'picasu-last-user-id'
const userId = ref(localStorage.getItem(LAST_USER_ID_KEY) ?? '')

const router = useRouter()
const messageStore = useMessageStore('mainId')
const constStore = useConstStore('mainId')
const vuetifyTheme = useTheme()

const themeIsLight = computed<boolean>({
  get: () => constStore.theme === 'light',
  set: () => {
    constStore.toggleTheme(vuetifyTheme).catch((err: unknown) => {
      console.error('Failed to update theme (via RegisterPage):', err)
    })
  }
})

// The form is only valid while no administrator is enrolled. A direct
// visit after enrollment (or on a closed install) leaves immediately:
// an enrolled store resolves to the timeline (the guard sorts out the
// session from there), anything else to the login screen.
onMounted(() => {
  void runEnrollmentBoot({
    probe: () => getEnrollmentProbe(),
    navigate: (destination) => {
      if (destination !== 'register') {
        void router.replace({ name: destination === 'app' ? 'timeline' : 'login' })
      }
    }
  })
})

// Form validation rules
const rules = {
  required: (value: string) => !!value || 'This field is required',
  matches: (value: string) => value === password.value || 'Passwords do not match'
}

const handleRegister = async () => {
  loading.value = true
  try {
    await tryWithMessageStore('mainId', async () => {
      await submitRegistration(userId.value, password.value, confirmPassword.value, {
        create: bootstrapUser,
        login,
        onRegistered: async (profile) => {
          // The session JWT is in an HttpOnly cookie the page never sees
          // (R5); keep only the display profile in the session store.
          useSessionStore('mainId').setProfile(profile)

          // Drop the cached empty-store probe so the next navigation
          // re-probes the now non-empty store instead of bouncing back
          // to `/register`.
          resetEnrollmentProbe()

          // Remember the id for the next login's User ID prefill.
          localStorage.setItem(LAST_USER_ID_KEY, userId.value)

          await router.replace({ name: 'timeline' })
        },
        onRaceLost: async (notice: string) => {
          // 409: another enrollment won the race, fall back to login.
          messageStore.info(notice)
          await router.replace({ name: 'login' })
        },
        onError: (err: unknown) => {
          messageStore.error(errorDisplay(err))
        }
      })
    })
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.register-card {
  max-width: 400px;
}

.theme-toggle-btn {
  position: fixed;
  top: 16px;
  right: 16px;
  z-index: 1000;
}

/* Responsive adjustments */
@media (max-width: 600px) {
  .register-card {
    margin: 16px;
  }
}
</style>
