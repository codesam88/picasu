import { describe, expect, test } from 'vitest'
import { AxiosError, type InternalAxiosRequestConfig } from 'axios'
import { loginFailureMessage } from './loginFailure'

const axiosErrorWithStatus = (status: number, message: string) => {
  const config = { url: '/post/users/authenticate' } as InternalAxiosRequestConfig
  return new AxiosError(message, 'ERR_BAD_REQUEST', config, null, {
    status,
    statusText: String(status),
    data: { message },
    headers: {},
    config
  })
}

describe('loginFailureMessage', () => {
  test('a 401 surfaces the fixed inline-banner copy', () => {
    expect(loginFailureMessage(axiosErrorWithStatus(401, 'Invalid credentials'))).toBe(
      'Invalid username or password'
    )
  })

  test('non-401s stay null (429 owns the countdown, the rest rethrow)', () => {
    expect(loginFailureMessage(axiosErrorWithStatus(429, 'Too many'))).toBeNull()
    expect(loginFailureMessage(axiosErrorWithStatus(500, 'boom'))).toBeNull()
    expect(loginFailureMessage(new Error('offline'))).toBeNull()
  })
})
