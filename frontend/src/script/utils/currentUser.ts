import { computed } from 'vue'
import type { ComputedRef } from 'vue'
import { useSessionStore } from '@/store/sessionStore'

/// Identity of the caller, from the session-store profile.
///
/// The session JWT lives in an HttpOnly cookie the page JS can never read,
/// so identity comes from the `{ userId, admin }` profile the login
/// response carried — never from decoding a token client-side.
export interface CurrentUser {
  id: string
  isAdmin: boolean
}

/// Whether the user-management panel is visible: admins only.
export function canSeeUserPanel(user: CurrentUser | null): boolean {
  return user?.isAdmin === true
}

/// Whether the SPA should load `/get/config`: that endpoint is
/// admin-or-share gated, so a logged-in non-admin would get a 401 that
/// the interceptor treats as a dead session. Viewers skip it and use
/// local defaults; everyone else (admin, share visitor, logged-out boot)
/// loads it.
export function shouldLoadAppConfig(user: CurrentUser | null): boolean {
  if (user !== null && !user.isAdmin) return false
  return true
}

/// State of a user row for the admin toggle:
/// - `'sole-admin'` — the row is the caller's own and no other admin exists,
///   so demoting would trip the server's unified zero-admin refusal.
/// - `'ok'` — the toggle is actionable (self-demotion with other admins
///   present is allowed: adminship transfer).
export function selfRowState(
  me: CurrentUser | null,
  targetUserId: string,
  adminCount: number
): 'ok' | 'sole-admin' {
  if (me !== null && me.id === targetUserId && adminCount <= 1) return 'sole-admin'
  return 'ok'
}

export type AdminErrorAction = { redirectLogin: true } | { redirectLogin: false; message: string }

/// Map a failed admin-toggle response to UI behavior: 401 means the session
/// is gone and the user must log in again, anything else surfaces the
/// server message verbatim.
export function errorToMessage(
  status: number | undefined,
  serverMessage?: string
): AdminErrorAction {
  if (status === 401) return { redirectLogin: true }
  if (serverMessage !== undefined && serverMessage !== '') {
    return { redirectLogin: false, message: serverMessage }
  }
  return { redirectLogin: false, message: 'Failed to update admin role' }
}

/// Current-user role state: maps the session-store profile (set at login)
/// to `{ id, isAdmin }`, or `null` when no session is known. Deliberately
/// not a second store — a thin view over the session store.
export function useCurrentUser(): ComputedRef<CurrentUser | null> {
  return computed(() => {
    const profile = useSessionStore('mainId').profile
    if (profile === null) return null
    return { id: profile.userId, isAdmin: profile.admin }
  })
}
