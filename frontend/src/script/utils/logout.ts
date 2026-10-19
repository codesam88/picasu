import { logout as logoutApi } from '@/api/users'
import { resetEnrollmentProbe } from './enrollment'
import { useRedirectionStore } from '@/store/redirectionStore'
import { useSessionStore } from '@/store/sessionStore'
import { useTokenStore } from '@/store/tokenStore'

export const performLogout = async (
  push: (location: { name: string }) => Promise<unknown>
): Promise<void> => {
  // The session cookie is HttpOnly, so only the server can clear it. Local
  // state is cleared unconditionally first, so logout sticks even when the
  // round trip fails (offline backend, dropped connection).
  useSessionStore('mainId').clearProfile()

  // Drop the cached probe (R1): it may hold an authenticated result that
  // would otherwise let the next navigation render app routes sessionless.
  resetEnrollmentProbe()

  const tokenStore = useTokenStore('mainId')
  tokenStore.timestampToken = null
  tokenStore.assetTokenMap.clear()

  const redirectionStore = useRedirectionStore('mainId')
  redirectionStore.redirection = null

  try {
    await logoutApi()
  } catch {
    // Local state above is already cleared; a failed round trip only means
    // the server-side cookie outlives us until expiry.
  }

  await push({ name: 'login' })
}
