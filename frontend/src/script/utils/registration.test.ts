import { describe, expect, test, vi } from 'vitest'
import { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import {
  RACE_LOST_NOTICE,
  initialRegisterUserId,
  isRegistrationSubmittable,
  submitRegistration,
  usernameFromQuery
} from './registration'

const axiosError = (status: number) => {
  const err = new Error(`Request failed with status code ${status}`) as Error & {
    response: { status: number }
  }
  err.response = { status }
  return err
}

/// Real axios rejection: `axios.isAxiosError` recognizes it, so the global
/// response interceptor owns its display and the local `onError` callback
/// must stay silent — except on the authenticate endpoint, where the
/// interceptor stays silent by design and the caller must display locally.
const realAxiosFailure = (
  status: number,
  url = '/post/users/bootstrap',
  body: unknown = { message: 'boom' }
) => {
  const config = { url, headers: {} } as InternalAxiosRequestConfig
  const response = {
    status,
    statusText: String(status),
    data: body,
    headers: {},
    config
  } as AxiosResponse
  return new AxiosError(
    `Request failed with status code ${status}`,
    'ERR_BAD_REQUEST',
    config,
    null,
    response
  )
}

const makeDeps = () => ({
  create: vi.fn().mockResolvedValue({ userId: 'root', admin: true }),
  login: vi.fn().mockResolvedValue({ userId: 'root', admin: true }),
  onRegistered: vi.fn().mockResolvedValue(undefined),
  onRaceLost: vi.fn().mockResolvedValue(undefined),
  onError: vi.fn()
})

describe('isRegistrationSubmittable', () => {
  test('blocks submit when password and confirm differ', () => {
    expect(isRegistrationSubmittable('root', 'new-secret', 'other-secret')).toBe(false)
  })

  test('blocks submit when the user id is empty', () => {
    expect(isRegistrationSubmittable('', 'new-secret', 'new-secret')).toBe(false)
  })

  test('blocks submit when the password is empty', () => {
    expect(isRegistrationSubmittable('root', '', '')).toBe(false)
  })

  test('allows submit when the id is non-empty and passwords match', () => {
    expect(isRegistrationSubmittable('root', 'new-secret', 'new-secret')).toBe(true)
  })
})

describe('submitRegistration', () => {
  test('mismatch never calls create', async () => {
    const deps = makeDeps()

    await expect(submitRegistration('root', 'new-secret', 'other-secret', deps)).resolves.toBe(
      'invalid'
    )
    expect(deps.create).not.toHaveBeenCalled()
    expect(deps.login).not.toHaveBeenCalled()
  })

  test('create then login then onRegistered with the profile', async () => {
    const deps = makeDeps()

    await expect(submitRegistration('root', 'new-secret', 'new-secret', deps)).resolves.toBe(
      'registered'
    )
    expect(deps.create).toHaveBeenCalledTimes(1)
    expect(deps.create).toHaveBeenCalledWith('root', 'new-secret')
    expect(deps.login).toHaveBeenCalledTimes(1)
    expect(deps.login).toHaveBeenCalledWith('root', 'new-secret')
    expect(deps.onRegistered).toHaveBeenCalledTimes(1)
    expect(deps.onRegistered).toHaveBeenCalledWith({ userId: 'root', admin: true })
    expect(deps.onRaceLost).not.toHaveBeenCalled()
    expect(deps.onError).not.toHaveBeenCalled()
  })

  test('409 on create falls back to the race-lost notice without login', async () => {
    const deps = makeDeps()
    deps.create = vi.fn().mockRejectedValueOnce(axiosError(409))

    await expect(submitRegistration('root', 'new-secret', 'new-secret', deps)).resolves.toBe(
      'race-lost'
    )
    expect(deps.login).not.toHaveBeenCalled()
    expect(deps.onRegistered).not.toHaveBeenCalled()
    expect(deps.onError).not.toHaveBeenCalled()
    expect(deps.onRaceLost).toHaveBeenCalledTimes(1)
    expect(deps.onRaceLost).toHaveBeenCalledWith(RACE_LOST_NOTICE)
  })

  test('non-409 create failure surfaces the error without redirect', async () => {
    const deps = makeDeps()
    const failure = axiosError(400)
    deps.create = vi.fn().mockRejectedValueOnce(failure)

    await expect(submitRegistration('root', 'new-secret', 'new-secret', deps)).resolves.toBe(
      'failed'
    )
    expect(deps.login).not.toHaveBeenCalled()
    expect(deps.onRegistered).not.toHaveBeenCalled()
    expect(deps.onRaceLost).not.toHaveBeenCalled()
    expect(deps.onError).toHaveBeenCalledTimes(1)
    expect(deps.onError).toHaveBeenCalledWith(failure)
  })

  test('login failure after create surfaces the error without redirect', async () => {
    const deps = makeDeps()
    const failure = axiosError(401)
    deps.login = vi.fn().mockRejectedValueOnce(failure)

    await expect(submitRegistration('root', 'new-secret', 'new-secret', deps)).resolves.toBe(
      'failed'
    )
    expect(deps.create).toHaveBeenCalledTimes(1)
    expect(deps.onRegistered).not.toHaveBeenCalled()
    expect(deps.onRaceLost).not.toHaveBeenCalled()
    expect(deps.onError).toHaveBeenCalledTimes(1)
    expect(deps.onError).toHaveBeenCalledWith(failure)
  })

  test('axios failure with a response never reaches onError (interceptor owns display)', async () => {
    const deps = makeDeps()
    deps.create = vi.fn().mockRejectedValueOnce(realAxiosFailure(400))

    await expect(submitRegistration('root', 'new-secret', 'new-secret', deps)).resolves.toBe(
      'failed'
    )
    expect(deps.login).not.toHaveBeenCalled()
    expect(deps.onRegistered).not.toHaveBeenCalled()
    expect(deps.onRaceLost).not.toHaveBeenCalled()
    expect(deps.onError).not.toHaveBeenCalled()
  })

  test('real axios 409 still falls back to the race-lost notice without onError', async () => {
    const deps = makeDeps()
    deps.create = vi.fn().mockRejectedValueOnce(realAxiosFailure(409))

    await expect(submitRegistration('root', 'new-secret', 'new-secret', deps)).resolves.toBe(
      'race-lost'
    )
    expect(deps.login).not.toHaveBeenCalled()
    expect(deps.onError).not.toHaveBeenCalled()
    expect(deps.onRaceLost).toHaveBeenCalledTimes(1)
    expect(deps.onRaceLost).toHaveBeenCalledWith(RACE_LOST_NOTICE)
  })

  test('responseless axios failure still surfaces via onError', async () => {
    const deps = makeDeps()
    const failure = new AxiosError(
      'Network Error',
      'ERR_NETWORK',
      { headers: {} } as InternalAxiosRequestConfig,
      null
    )
    deps.create = vi.fn().mockRejectedValueOnce(failure)

    await expect(submitRegistration('root', 'new-secret', 'new-secret', deps)).resolves.toBe(
      'failed'
    )
    expect(deps.onError).toHaveBeenCalledTimes(1)
    expect(deps.onError).toHaveBeenCalledWith(failure)
  })

  test('non-axios throw still surfaces via onError', async () => {
    const deps = makeDeps()
    const failure = new Error('boom')
    deps.create = vi.fn().mockRejectedValueOnce(failure)

    await expect(submitRegistration('root', 'new-secret', 'new-secret', deps)).resolves.toBe(
      'failed'
    )
    expect(deps.onError).toHaveBeenCalledTimes(1)
    expect(deps.onError).toHaveBeenCalledWith(failure)
  })

  test('throttled register-login round trip surfaces via onError exactly once', async () => {
    // The login step hits /post/users/authenticate, where the interceptor stays
    // silent by design — so the local snackbar is the only signal and must
    // fire. Both hinted and hint-less 429s take this path: the register
    // page has no countdown banner.
    for (const failure of [
      realAxiosFailure(429, '/post/users/authenticate', {
        kind: 'Rate Limited',
        message: 'Too many login attempts, try again in 10s',
        status: 'Temporary',
        context: [],
        retryAfter: 10
      }),
      realAxiosFailure(429, '/post/users/authenticate', {
        kind: 'Rate Limited',
        message: 'Too many login attempts',
        status: 'Temporary',
        context: []
      })
    ]) {
      const deps = makeDeps()
      deps.login = vi.fn().mockRejectedValueOnce(failure)

      await expect(submitRegistration('root', 'new-secret', 'new-secret', deps)).resolves.toBe(
        'failed'
      )
      expect(deps.create).toHaveBeenCalledTimes(1)
      expect(deps.onRegistered).not.toHaveBeenCalled()
      expect(deps.onRaceLost).not.toHaveBeenCalled()
      expect(deps.onError).toHaveBeenCalledTimes(1)
      expect(deps.onError).toHaveBeenCalledWith(failure)
    }
  })

  test('429 on the create step stays swallowed (interceptor toasts those)', async () => {
    const deps = makeDeps()
    deps.create = vi.fn().mockRejectedValueOnce(realAxiosFailure(429))

    await expect(submitRegistration('root', 'new-secret', 'new-secret', deps)).resolves.toBe(
      'failed'
    )
    expect(deps.login).not.toHaveBeenCalled()
    expect(deps.onError).not.toHaveBeenCalled()
  })
})

describe('usernameFromQuery', () => {
  test('returns the trimmed username when non-empty', () => {
    expect(usernameFromQuery({ username: ' maria ' })).toBe('maria')
  })

  test('returns undefined for missing or empty values', () => {
    expect(usernameFromQuery({})).toBeUndefined()
    expect(usernameFromQuery({ username: '   ' })).toBeUndefined()
    expect(usernameFromQuery({ username: 42 as unknown as string })).toBeUndefined()
  })
})

describe('initialRegisterUserId', () => {
  test('invite mode prefers the username query param over last-user-id', () => {
    expect(
      initialRegisterUserId({ invite: 'tok', usernameParam: 'maria', lastUserId: 'old' })
    ).toBe('maria')
  })

  test('invite mode without a username param starts empty, not last-user-id', () => {
    expect(initialRegisterUserId({ invite: 'tok', lastUserId: 'old' })).toBe('')
  })

  test('non-invite mode keeps the last-user-id prefill', () => {
    expect(initialRegisterUserId({ lastUserId: 'old' })).toBe('old')
  })
})
