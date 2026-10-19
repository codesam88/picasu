import { describe, expect, test, vi } from 'vitest'
import { RACE_LOST_NOTICE, isRegistrationSubmittable, submitRegistration } from './registration'

const axiosError = (status: number) => {
  const err = new Error(`Request failed with status code ${status}`) as Error & {
    response: { status: number }
  }
  err.response = { status }
  return err
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
})
