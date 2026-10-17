import { beforeAll, beforeEach, describe, expect, test, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import axios, { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import { setupMainAxiosInterceptor } from './axiosInterceptor'
import { useMessageStore } from '@/store/messageStore'
import { useSessionStore } from '@/store/sessionStore'
import { useRedirectionStore } from '@/store/redirectionStore'

// The router module imports .vue page components, which the node-env vitest
// setup cannot parse (same reason routes.test.ts checks source instead of
// importing it). Stub only the router; the real pinia stores under test still
// load (see logout.test.ts for the same pattern).
vi.mock('@/route/routes', () => ({
  default: {
    currentRoute: { value: { name: 'timeline', fullPath: '/timeline' } },
    push: vi.fn()
  }
}))

beforeAll(() => {
  setupMainAxiosInterceptor()
})

beforeEach(() => {
  setActivePinia(createPinia())
})

/// Route every request through an adapter that rejects with the given
/// axios-shaped response, so the registered response interceptor runs the
/// same code path it would for a real 429.
const rejectWith = (status: number, headers: Record<string, string>, body: unknown) => {
  axios.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
    const response: AxiosResponse = {
      status,
      statusText: String(status),
      data: body,
      headers,
      config
    }
    throw new AxiosError(
      `Request failed with status code ${status}`,
      'ERR_BAD_REQUEST',
      config,
      null,
      response
    )
  }
}

const rateLimitBody = (retryAfter?: number) => ({
  kind: 'Rate Limited',
  message: 'Too many login attempts',
  status: 429,
  context: null,
  ...(retryAfter !== undefined ? { retryAfter } : {})
})

describe('429 handling', () => {
  test('authenticate 429 emits no toast, clears no profile, redirects nowhere', async () => {
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: true })
    const redirectionStore = useRedirectionStore('mainId')
    const redirectSpy = vi.spyOn(redirectionStore, 'redirectionToLogin')
    rejectWith(429, { 'retry-after': '2' }, rateLimitBody(2))

    await expect(
      axios.post('/post/authenticate', { userId: 'alice', password: 'wrong' })
    ).rejects.toBeTruthy()

    expect(useMessageStore('mainId').queue).toHaveLength(0)
    expect(useSessionStore('mainId').profile).toEqual({ userId: 'alice', admin: true })
    expect(redirectSpy).not.toHaveBeenCalled()
  })

  test('throttled authenticate 429 still emits no toast when the retry hint is unparsable', async () => {
    rejectWith(429, {}, rateLimitBody())

    await expect(
      axios.post('/post/authenticate', { userId: 'alice', password: 'wrong' })
    ).rejects.toBeTruthy()

    expect(useMessageStore('mainId').queue).toHaveLength(0)
  })

  test('429 from another route toasts the retry message exactly once', async () => {
    rejectWith(429, { 'retry-after': '7' }, rateLimitBody(7))

    await expect(axios.post('/get/get-data', {})).rejects.toBeTruthy()

    const queue = useMessageStore('mainId').queue
    expect(queue).toHaveLength(1)
    expect(queue[0]?.text).toContain('try again in 7s')
    expect(queue[0]?.color).toBe('error')
  })

  test('429 from another route falls back to the body retryAfter field', async () => {
    rejectWith(429, {}, rateLimitBody(5))

    await expect(axios.post('/put/config', {})).rejects.toBeTruthy()

    const queue = useMessageStore('mainId').queue
    expect(queue).toHaveLength(1)
    expect(queue[0]?.text).toContain('try again in 5s')
  })
})
