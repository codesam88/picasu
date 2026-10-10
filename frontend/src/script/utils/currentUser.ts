import { computed } from 'vue'
import type { ComputedRef } from 'vue'
import Cookies from 'js-cookie'
import { jwtDecode } from 'jwt-decode'

/// Identity of the caller, decoded from the readable `jwt` session cookie.
///
/// The backend mints externally-tagged role claims: a user session carries
/// `role: { user: { id, admin } }`, while share sessions carry
/// `role: { share: ... }`. Only the user shape yields a `CurrentUser`.
export interface CurrentUser {
  id: string
  isAdmin: boolean
}

/// Decode a session token into `{ id, isAdmin }`, or `null` when the token
/// is missing, unparseable, or carries a non-user role (e.g. share tokens).
export function parseCurrentUser(token: string | undefined): CurrentUser | null {
  if (token === undefined || token === '') return null
  try {
    const decoded = jwtDecode<{ role?: { user?: { id?: unknown; admin?: unknown } } }>(token)
    const user = decoded?.role?.user
    if (typeof user?.id === 'string' && typeof user?.admin === 'boolean') {
      return { id: user.id, isAdmin: user.admin }
    }
    return null
  } catch {
    return null
  }
}

/// Whether the user-management panel is visible: admins only.
export function canSeeUserPanel(user: CurrentUser | null): boolean {
  return user?.isAdmin === true
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

/// Lazy current-user role state: reads and decodes the `jwt` cookie on each
/// access inside a `computed`, so the role is always fresh with no hydration
/// wiring to rot. Deliberately not a store.
export function useCurrentUser(): ComputedRef<CurrentUser | null> {
  return computed(() => parseCurrentUser(Cookies.get('jwt')))
}
