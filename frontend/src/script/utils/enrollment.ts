import { listUsers, type UserSummary } from '@/api/users'

export type BootDestination = 'register' | 'login' | 'app'

export type ProbeResult = { ok: true; users: UserSummary[] } | { ok: false; status?: number }

/// Map a `GET /get/users` probe outcome to the boot destination: an empty
/// store means no administrator is enrolled yet so the registration screen
/// takes over; any probe failure (401 on a closed install or any other
/// error) falls back to the normal login.
export const decideBootDestination = (result: ProbeResult): BootDestination => {
  if (result.ok) {
    return result.users.length === 0 ? 'register' : 'app'
  }
  return 'login'
}

/// Probe once whether any user exists. Failures resolve to a not-ok
/// result carrying the HTTP status when one is available; they never throw.
export const probeEnrollment = async (): Promise<ProbeResult> => {
  try {
    return { ok: true, users: await listUsers() }
  } catch (err: unknown) {
    return { ok: false, status: (err as { response?: { status?: number } })?.response?.status }
  }
}

/// Run the boot probe once and navigate to the decided destination.
/// Navigation is injected so the orchestration stays unit-testable.
export const runEnrollmentBoot = async (deps: {
  probe: () => Promise<ProbeResult>
  navigate: (destination: BootDestination) => Promise<void> | void
}): Promise<BootDestination> => {
  const destination = decideBootDestination(await deps.probe())
  await deps.navigate(destination)
  return destination
}
