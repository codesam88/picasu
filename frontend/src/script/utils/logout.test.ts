import { describe, expect, test, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import axios from 'axios'
import { useSessionStore } from '@/store/sessionStore'
import { useTokenStore } from '@/store/tokenStore'
import { useRedirectionStore } from '@/store/redirectionStore'
import { performLogout } from './logout'

vi.mock('axios')

const mockedAxios = axios as unknown as {
  post: ReturnType<typeof vi.fn>
}

// The router module imports .vue page components, which the node-env vitest
// setup cannot parse (same reason routes.test.ts checks source instead of
// importing it). Stub only the router; the real pinia stores under test still
// load and are seeded/asserted below.
vi.mock('@/route/routes', () => ({
  default: {
    currentRoute: { value: { name: 'login', fullPath: '/login' } },
    push: vi.fn()
  }
}))

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  mockedAxios.post = vi.fn().mockResolvedValue({ data: '' })
})

const push = () => vi.fn().mockResolvedValue(undefined)

describe('performLogout', () => {
  test('clears the session cookie server-side via POST /post/logout', async () => {
    await performLogout(push())

    expect(mockedAxios.post).toHaveBeenCalledTimes(1)
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/logout')
  })

  test('clears the session profile', async () => {
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: true })

    await performLogout(push())

    expect(useSessionStore('mainId').profile).toBeNull()
  })

  test('resets tokenStore state', async () => {
    const tokenStore = useTokenStore('mainId')
    tokenStore.timestampToken = 'some-token'
    tokenStore.assetTokenMap.set('asset_a', 'token_a')

    await performLogout(push())

    expect(tokenStore.timestampToken).toBeNull()
    expect(tokenStore.assetTokenMap.size).toBe(0)
  })

  test('clears the redirection store', async () => {
    const redirectionStore = useRedirectionStore('mainId')
    redirectionStore.redirection = '/timeline'

    await performLogout(push())

    expect(redirectionStore.redirection).toBeNull()
  })

  test('still clears local state when the logout round trip fails', async () => {
    mockedAxios.post = vi.fn().mockRejectedValueOnce(new Error('Network Error'))
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: true })

    const pushMock = push()
    await performLogout(pushMock)

    expect(useSessionStore('mainId').profile).toBeNull()
    expect(pushMock).toHaveBeenCalledWith({ name: 'login' })
  })

  test('resets the cached enrollment probe', async () => {
    const { getEnrollmentProbe, resetEnrollmentProbe } = await import('./enrollment')
    resetEnrollmentProbe()
    await getEnrollmentProbe(() => Promise.resolve({ ok: false }))

    await performLogout(push())

    // A new injectable probe runs only when the logout cleared the cache.
    await expect(
      getEnrollmentProbe(() => Promise.resolve({ ok: true, users: [] }))
    ).resolves.toEqual({ ok: true, users: [] })
  })

  test('navigates to login', async () => {
    const pushMock = push()

    await performLogout(pushMock)

    expect(pushMock).toHaveBeenCalledTimes(1)
    expect(pushMock).toHaveBeenCalledWith({ name: 'login' })
  })
})
