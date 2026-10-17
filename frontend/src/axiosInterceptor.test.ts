import { beforeAll, beforeEach, describe, expect, test, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import axios, { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import { setupMainAxiosInterceptor } from './axiosInterceptor'
import { useMessageStore } from '@/store/messageStore'
import { useSessionStore } from '@/store/sessionStore'
import { useRedirectionStore } from '@/store/redirectionStore'
import router from '@/route/routes'

// The router module imports .vue page components, which the node-env vitest
// setup cannot parse (same reason routes.test.ts checks source instead of
// importing it). Stub only the router; the real pinia stores under test still
// load (see logout.test.ts for the same pattern).
// Navigating stub: landing on login flips currentRoute, mirroring the real
// router — that flip is what makes repeat redirectionToLogin calls no-ops.
vi.mock('@/route/routes', () => {
  const currentRoute = { value: { name: 'timeline', fullPath: '/timeline' } }
  const push = vi.fn(async () => {
    currentRoute.value = { name: 'login', fullPath: '/login' }
  })
  return {
    default: {
      currentRoute,
      push
    }
  }
})

/// Typed view of the router stub above (the real router types are unusable:
/// the module is mocked and .vue routes never load under node vitest).
const mockedRouter = router as unknown as {
  currentRoute: { value: { name: string; fullPath: string } }
  push: ReturnType<typeof vi.fn>
}

beforeAll(() => {
  setupMainAxiosInterceptor()
})

beforeEach(() => {
  setActivePinia(createPinia())
  mockedRouter.currentRoute.value = { name: 'timeline', fullPath: '/timeline' }
  mockedRouter.push.mockClear()
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

/// Backend `AppError` body shape for 429 answers (see error.rs): the
/// `status` is the wire string, `context` the list — never the numeric
/// code or null.
const rateLimitBody = (retryAfter?: number) => ({
  kind: 'Rate Limited',
  message: 'Too many login attempts',
  status: 'Temporary',
  context: [] as string[],
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

  test('429 from another route without any hint toasts the server message', async () => {
    rejectWith(
      429,
      {},
      { kind: 'Rate Limited', message: 'Too many login attempts', status: 'Temporary', context: [] }
    )

    await expect(axios.post('/put/config', {})).rejects.toBeTruthy()

    const queue = useMessageStore('mainId').queue
    expect(queue).toHaveLength(1)
    expect(queue[0]?.text).toContain('Too many login attempts')
  })

  test('unlisted statuses (e.g. 409) toast the server message exactly once', async () => {
    rejectWith(409, {}, { message: 'User already exists' })

    await expect(axios.post('/post/users/bootstrap', {})).rejects.toBeTruthy()

    const queue = useMessageStore('mainId').queue
    expect(queue).toHaveLength(1)
    expect(queue[0]?.text).toContain('User already exists')
    expect(queue[0]?.color).toBe('error')
  })
})

describe('401 handling', () => {
  test('clears the session profile', async () => {
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: true })
    rejectWith(401, {}, {})

    await expect(axios.get('/get/get-data')).rejects.toBeTruthy()

    expect(useSessionStore('mainId').profile).toBeNull()
  })

  test('parallel 401s navigate to login exactly once', async () => {
    rejectWith(401, {}, {})

    const outcomes = await Promise.allSettled([
      axios.get('/get/get-data'),
      axios.get('/get/get-data'),
      axios.get('/get/get-data')
    ])
    expect(outcomes.filter((outcome) => outcome.status === 'rejected')).toHaveLength(3)

    expect(mockedRouter.push).toHaveBeenCalledTimes(1)
    expect(mockedRouter.push).toHaveBeenCalledWith({ name: 'login' })
  })

  test('emits no snackbar for a 401', async () => {
    rejectWith(401, {}, {})

    await expect(axios.get('/get/get-data')).rejects.toBeTruthy()

    expect(useMessageStore('mainId').queue).toHaveLength(0)
  })

  test('a rapid second 401 does not navigate again', async () => {
    rejectWith(401, {}, {})

    await expect(axios.get('/get/get-data')).rejects.toBeTruthy()
    await expect(axios.get('/get/get-data')).rejects.toBeTruthy()

    expect(mockedRouter.push).toHaveBeenCalledTimes(1)
  })
})
