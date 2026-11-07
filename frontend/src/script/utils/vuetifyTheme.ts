import type { ThemeName } from '@/db/settingsDb'

/// Structural slice of Vuetify's theme instance (what `useTheme()` returns)
/// that the app mutates to switch the live theme.
interface VuetifyThemeHandle {
  global: { name: { value: string } }
}

let registered: VuetifyThemeHandle | null = null

/// Register the app-level Vuetify theme instance at boot (main.ts), so
/// identity changes outside component context — logout, 401 interceptor —
/// can apply the reloaded theme where `useTheme()` has no injection scope.
export function registerVuetifyTheme(theme: VuetifyThemeHandle): void {
  registered = theme
}

/// Apply `name` to the registered Vuetify theme; no-op until main.ts
/// registers (node tests, pre-boot).
export function applyVuetifyTheme(name: ThemeName): void {
  if (registered !== null) {
    registered.global.name.value = name
  }
}
