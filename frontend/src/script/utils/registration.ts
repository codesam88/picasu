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
  login: (userId: string, password: string) => Promise<string>
  onRegistered: (token: string) => Promise<void> | void
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
    const token = await deps.login(userId, password)
    await deps.onRegistered(token)
    return 'registered'
  } catch (err: unknown) {
    if ((err as { response?: { status?: number } })?.response?.status === 409) {
      await deps.onRaceLost(RACE_LOST_NOTICE)
      return 'race-lost'
    }
    deps.onError(err)
    return 'failed'
  }
}
