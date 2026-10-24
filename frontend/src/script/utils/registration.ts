import type { UserSummary } from '@/api/users'
import axios from 'axios'
import { isAuthenticateThrottle } from './retryAfter'

/// Extract a non-empty invite token from a route query object. Returns
/// `undefined` when the value is missing, not a string, or empty, so callers
/// can treat the result as a simple "invite mode?" flag.
export const inviteFromQuery = (
  query: { invite?: unknown } | null | undefined
): string | undefined => {
  const value = query?.invite
  return typeof value === 'string' && value !== '' ? value : undefined
}

export const isRegistrationSubmittable = (
  userId: string,
  password: string,
  confirmPassword: string
): boolean => {
  return userId.length > 0 && password.length > 0 && password === confirmPassword
}

export type RegistrationOutcome = 'registered' | 'race-lost' | 'invalid' | 'failed'

export interface RegistrationDeps {
  create: (userId: string, password: string) => Promise<unknown>
  login: (userId: string, password: string) => Promise<UserSummary>
  onRegistered: (profile: UserSummary) => Promise<void> | void
  onRaceLost: (notice: string) => Promise<void> | void
  onError: (error: unknown) => void
}

/// Shown when the create races with another enrollment and loses (409):
/// the account already exists, so the user should log in instead.
export const RACE_LOST_NOTICE = 'An administrator account already exists. Please log in instead.'

/// Run the first-user registration: validate client-side, create the
/// user, then log in. A 409 means another enrollment won the race and
/// falls back to the login notice; any other failure surfaces the error
/// with no redirect. Redirects stay in the injected callbacks so the
/// branching is unit-testable without a router.
export const submitRegistration = async (
  userId: string,
  password: string,
  confirmPassword: string,
  deps: RegistrationDeps
): Promise<RegistrationOutcome> => {
  if (!isRegistrationSubmittable(userId, password, confirmPassword)) return 'invalid'
  try {
    await deps.create(userId, password)
    const profile = await deps.login(userId, password)
    await deps.onRegistered(profile)
    return 'registered'
  } catch (err: unknown) {
    if ((err as { response?: { status?: number } })?.response?.status === 409) {
      await deps.onRaceLost(RACE_LOST_NOTICE)
      return 'race-lost'
    }
    // The interceptor stays silent for authenticate 429s by design (the
    // login page owns that display inline); the register page has no
    // banner, so surface the throttled login locally exactly once via
    // onError. Checked before the swallow below: those errors do carry a
    // response, but nobody else displays them.
    if (isAuthenticateThrottle(err)) {
      deps.onError(err)
      return 'failed'
    }
    // The global axios interceptor already displayed axios errors that
    // carry a response; surface only what it could not (non-axios throws
    // and responseless failures).
    if (!axios.isAxiosError(err) || err.response === undefined) {
      deps.onError(err)
    }
    return 'failed'
  }
}
