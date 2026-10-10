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
        <v-card class="login-card mx-auto" elevation="8" rounded="lg">
          <v-card-text class="pa-8">
            <div class="text-center mb-6">
              <h2 class="text-h4 font-weight-light mb-2">Welcome Back!</h2>
            </div>

            <v-form @submit.prevent="handleLogin" ref="form">
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

              <v-btn type="submit" color="primary" size="large" block :loading="loading">
                Login
              </v-btn>
            </v-form>
          </v-card-text>
        </v-card>
      </v-col>
    </v-row>
  </v-container>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import Cookies from 'js-cookie'
import { useRouter } from 'vue-router'
import { useRedirectionStore } from '@/store/redirectionStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { useConstStore } from '@/store/constStore'
import { useTheme } from 'vuetify'
import { login } from '@/api/users'

const password = ref('')
const showPassword = ref(false)
const loading = ref(false)

// Prefill from the last successful login; empty on first run.
const LAST_USER_ID_KEY = 'picasu-last-user-id'
const userId = ref(localStorage.getItem(LAST_USER_ID_KEY) ?? '')

const router = useRouter()
const redirectionStore = useRedirectionStore('mainId')
const constStore = useConstStore('mainId')
const vuetifyTheme = useTheme()

const themeIsLight = computed<boolean>({
  get: () => constStore.theme === 'light',
  set: () => {
    constStore.toggleTheme(vuetifyTheme).catch((err: unknown) => {
      console.error('Failed to update theme (via LoginPage):', err)
    })
  }
})

// Form validation rules
const rules = {
  required: (value: string) => !!value || 'This field is required'
}

const handleLogin = async () => {
  loading.value = true
  try {
    await tryWithMessageStore('mainId', async () => {
      const tokenValue = await login(userId.value, password.value)

      // Store the JWT in a cookie with security attributes
      Cookies.set('jwt', tokenValue, {
        httpOnly: false, // Set to true for better security (cannot access via JavaScript)
        secure: true, // Ensure it's only sent over HTTPS
        sameSite: 'Strict', // Prevent CSRF attacks
        expires: 14 // Optional: Expires in 1 day
      })

      // Remember the id for the next login's User ID prefill.
      localStorage.setItem(LAST_USER_ID_KEY, userId.value)

      const redirection = redirectionStore.redirection
      if (redirection !== null) {
        // We have a `redirection` value:
        // This means the user was actually on some app page (e.g. A) and then navigated to `/login`.
        // At this point, the session history looks like: [..., A, /login]
        // After a successful login, we simply go back one step to return to A.
        // This also ensures that when the user presses the browser Back button again,
        // they will not go back to `/login` but to the page before A.
        router.back()
      } else {
        // No `redirection`:
        // This means the user opened `/login` directly (not from an internal app page).
        // In this case, there is no "previous app page" to go back to, so after a successful login
        // we redirect the user to the home page.
        // Using `replace` here removes `/login` from the current session history entry,
        // so the Back button will not return to the login page.
        await router.replace({ name: 'timeline' })
      }
    })
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.login-card {
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
  .login-card {
    margin: 16px;
  }
}
</style>
