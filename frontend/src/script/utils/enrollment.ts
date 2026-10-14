import axios, { type AxiosResponse } from 'axios'
import { type UserSummary } from '@/api/users'

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
///
/// The probe runs on every boot — including logged-out, where `GET
/// /get/users` answers 401 — so it opts out of axios rejection with
/// `validateStatus: () => true` and branches on the status itself. A rejected
/// error status would first run the global `handleAxiosResponseError`
/// (session-expired toast + login redirect) before the boot guard routes on
/// the result; a resolved status keeps the probe silent.
export const probeEnrollment = async (): Promise<ProbeResult> => {
  let response: AxiosResponse<UserSummary[]>
  try {
    response = await axios.get<UserSummary[]>('/get/users', { validateStatus: () => true })
  } catch (err: unknown) {
    return { ok: false, status: (err as { response?: { status?: number } })?.response?.status }
  }
  if (response.status >= 200 && response.status < 300) {
    return { ok: true, users: response.data }
  }
  return { ok: false, status: response.status }
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

/// Cached boot probe shared with the router guard. The guard resolves it
/// once per app start; after a successful registration the store is
/// non-empty but the cache still resolves `[]`, so every later navigation
/// would bounce back to `/register`. Registration calls
/// `resetEnrollmentProbe` on success so the next navigation re-probes and
/// sees the enrolled store.
let cachedEnrollmentProbe: Promise<ProbeResult> | null = null

/// Return the cached probe, starting it on first use. The `probe`
/// parameter is injectable so the caching stays unit-testable; production
/// callers use the default live probe.
export const getEnrollmentProbe = (
  probe: () => Promise<ProbeResult> = probeEnrollment
): Promise<ProbeResult> => {
  if (cachedEnrollmentProbe === null) {
    cachedEnrollmentProbe = probe()
  }
  return cachedEnrollmentProbe
}

/// Drop the cached probe so the next read re-probes. Safe to call when
/// no probe is cached.
export const resetEnrollmentProbe = (): void => {
  cachedEnrollmentProbe = null
}
