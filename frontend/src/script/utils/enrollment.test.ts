import { describe, expect, test, vi, beforeEach } from 'vitest'
import axios from 'axios'
import { decideBootDestination, probeEnrollment, runEnrollmentBoot } from './enrollment'

vi.mock('axios')

const mockedAxios = axios as unknown as {
  get: ReturnType<typeof vi.fn>
}

beforeEach(() => {
  vi.resetAllMocks()
})

describe('decideBootDestination', () => {
  test('empty probe list forces registration', () => {
    expect(decideBootDestination({ ok: true, users: [] })).toBe('register')
  })

  test('non-empty probe list enters the normal app', () => {
    expect(decideBootDestination({ ok: true, users: [{ userId: 'root', admin: true }] })).toBe(
      'app'
    )
  })

  test('probe 401 goes to login', () => {
    expect(decideBootDestination({ ok: false, status: 401 })).toBe('login')
  })

  test('probe failure without a status goes to login', () => {
    expect(decideBootDestination({ ok: false })).toBe('login')
  })
})

describe('probeEnrollment', () => {
  test('maps GET /get/users [] to an ok empty result', async () => {
    mockedAxios.get = vi.fn().mockResolvedValue({ status: 200, data: [] })
    await expect(probeEnrollment()).resolves.toEqual({ ok: true, users: [] })
    expect(mockedAxios.get).toHaveBeenCalledWith('/get/users', {
      validateStatus: expect.any(Function)
    })
  })

  test('maps a non-empty list to an ok result', async () => {
    mockedAxios.get = vi
      .fn()
      .mockResolvedValue({ status: 200, data: [{ userId: 'root', admin: true }] })
    await expect(probeEnrollment()).resolves.toEqual({
      ok: true,
      users: [{ userId: 'root', admin: true }]
    })
  })

  test('maps a 401 rejection to a not-ok result with status', async () => {
    const err = new Error('Request failed with status code 401') as Error & {
      response: { status: number }
    }
    err.response = { status: 401 }
    mockedAxios.get = vi.fn().mockRejectedValueOnce(err)
    await expect(probeEnrollment()).resolves.toEqual({ ok: false, status: 401 })
  })

  test('maps a status-less failure to a not-ok result without status', async () => {
    mockedAxios.get = vi.fn().mockRejectedValueOnce(new Error('Network Error'))
    await expect(probeEnrollment()).resolves.toEqual({ ok: false, status: undefined })
  })

  test('maps a resolved 401 response to a not-ok result with status', async () => {
    mockedAxios.get = vi.fn().mockResolvedValue({ status: 401, data: '' })
    await expect(probeEnrollment()).resolves.toEqual({ ok: false, status: 401 })
  })

  test('maps a resolved 422 response to a not-ok result with status', async () => {
    mockedAxios.get = vi.fn().mockResolvedValue({ status: 422, data: '' })
    await expect(probeEnrollment()).resolves.toEqual({ ok: false, status: 422 })
  })

  test('probe GET opts out of rejection so the interceptor stays silent', async () => {
    mockedAxios.get = vi.fn().mockResolvedValue({ status: 200, data: [] })
    await probeEnrollment()
    const config = (mockedAxios.get.mock.calls[0] as unknown[])[1] as
      { validateStatus?: (status: number) => boolean } | undefined
    expect(typeof config?.validateStatus).toBe('function')
    // Error statuses resolve instead of rejecting, so the global
    // handleAxiosResponseError (toast + login redirect) never runs.
    expect(config?.validateStatus?.(401)).toBe(true)
    expect(config?.validateStatus?.(422)).toBe(true)
    expect(config?.validateStatus?.(500)).toBe(true)
  })
})

describe('runEnrollmentBoot', () => {
  test('probes once and navigates to the decided destination', async () => {
    const probe = vi.fn().mockResolvedValue({ ok: true, users: [] })
    const navigate = vi.fn().mockResolvedValue(undefined)
    await expect(runEnrollmentBoot({ probe, navigate })).resolves.toBe('register')
    expect(probe).toHaveBeenCalledTimes(1)
    expect(navigate).toHaveBeenCalledTimes(1)
    expect(navigate).toHaveBeenCalledWith('register')
  })

  test('non-empty probe navigates into the app', async () => {
    const probe = vi.fn().mockResolvedValue({ ok: true, users: [{ userId: 'root', admin: true }] })
    const navigate = vi.fn().mockResolvedValue(undefined)
    await expect(runEnrollmentBoot({ probe, navigate })).resolves.toBe('app')
    expect(navigate).toHaveBeenCalledWith('app')
  })

  test('failed probe navigates to login', async () => {
    const probe = vi.fn().mockResolvedValue({ ok: false, status: 401 })
    const navigate = vi.fn().mockResolvedValue(undefined)
    await expect(runEnrollmentBoot({ probe, navigate })).resolves.toBe('login')
    expect(navigate).toHaveBeenCalledWith('login')
  })
})
