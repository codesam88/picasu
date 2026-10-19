import { IsolationId } from '@type/types'
import { defineStore } from 'pinia'
import { z } from 'zod'
import type { UserSummary } from '@/api/users'

/// localStorage key for the session profile. The JWT itself lives in an
/// HttpOnly cookie the page JS can never read (R5); this persisted profile
/// is display-only (admin UI gating) — the backend re-reads the role from
/// the user table on every request, so a stale profile grants nothing.
export const SESSION_PROFILE_KEY = 'picasu-session-profile'

const profileSchema = z.object({ userId: z.string(), admin: z.boolean() })

/// Read the persisted profile, or `null` when absent or malformed.
/// Corrupt values clear themselves so one bad write cannot wedge the UI.
function loadProfile(): UserSummary | null {
  try {
    const raw = localStorage.getItem(SESSION_PROFILE_KEY)
    if (raw === null) return null
    const parsed: unknown = JSON.parse(raw)
    const result = profileSchema.safeParse(parsed)
    if (!result.success) {
      localStorage.removeItem(SESSION_PROFILE_KEY)
      return null
    }
    return result.data
  } catch {
    return null
  }
}

/// Client-side session identity: the profile returned by login/register.
///
/// Hydrated from localStorage so it survives reloads (the cookie alone
/// cannot tell the UI *who* is logged in). Cleared on logout and on any
/// 401, which bounds display staleness after server-side demotion/removal:
/// the next render may briefly show the old role, but the first guarded
/// request 401s, clears the profile, and lands on login.
export const useSessionStore = (isolationId: IsolationId) =>
  defineStore('sessionStore' + isolationId, {
    state: (): {
      profile: UserSummary | null
    } => ({
      profile: loadProfile()
    }),
    actions: {
      setProfile(profile: UserSummary) {
        this.profile = profile
        try {
          localStorage.setItem(SESSION_PROFILE_KEY, JSON.stringify(profile))
        } catch {
          // Storage full or unavailable (private mode): the in-memory
          // profile still works for this page lifetime.
        }
      },
      clearProfile() {
        this.profile = null
        try {
          localStorage.removeItem(SESSION_PROFILE_KEY)
        } catch {
          // Clearing is best-effort; the in-memory null already gates UI.
        }
      }
    }
  })()
