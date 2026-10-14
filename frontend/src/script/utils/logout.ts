import { useRedirectionStore } from '@/store/redirectionStore'
import { useTokenStore } from '@/store/tokenStore'
import { clearSessionToken } from './sessionCookie'

export const performLogout = async (
  push: (location: { name: string }) => Promise<unknown>
): Promise<void> => {
  clearSessionToken()

  const tokenStore = useTokenStore('mainId')
  tokenStore.timestampToken = null
  tokenStore.assetTokenMap.clear()

  const redirectionStore = useRedirectionStore('mainId')
  redirectionStore.redirection = null

  await push({ name: 'login' })
}
