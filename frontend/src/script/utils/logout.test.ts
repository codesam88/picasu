import { describe, expect, test, vi, beforeEach, type Mock } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import Cookies from 'js-cookie'
import { useTokenStore } from '@/store/tokenStore'
import { useRedirectionStore } from '@/store/redirectionStore'
import { performLogout } from './logout'

vi.mock('js-cookie', () => ({
  default: {
    set: vi.fn(),
    remove: vi.fn()
  }
}))

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

const mockedCookies = Cookies as unknown as {
  remove: Mock
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
})

const push = () => vi.fn().mockResolvedValue(undefined)

describe('performLogout', () => {
  test('removes the session cookie', async () => {
    await performLogout(push())

    expect(mockedCookies.remove).toHaveBeenCalledTimes(1)
    expect(mockedCookies.remove.mock.calls[0]?.[0]).toBe('jwt')
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

  test('navigates to login', async () => {
    const pushMock = push()

    await performLogout(pushMock)

    expect(pushMock).toHaveBeenCalledTimes(1)
    expect(pushMock).toHaveBeenCalledWith({ name: 'login' })
  })
})
