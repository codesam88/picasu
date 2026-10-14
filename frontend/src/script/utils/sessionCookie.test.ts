import { describe, expect, test, vi, beforeEach, type Mock } from 'vitest'
import Cookies from 'js-cookie'
import {
  SESSION_COOKIE_NAME,
  clearSessionToken,
  resolveSessionCookieAttributes,
  setSessionToken
} from './sessionCookie'

vi.mock('js-cookie', () => ({
  default: {
    set: vi.fn(),
    remove: vi.fn()
  }
}))

const mockedCookies = Cookies as unknown as {
  set: Mock
  remove: Mock
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('sessionCookie', () => {
  test('session cookie uses the jwt name', () => {
    expect(SESSION_COOKIE_NAME).toBe('jwt')
  })

  test('shared attributes carry path, sameSite, secure and 14-day expiry', () => {
    expect(resolveSessionCookieAttributes('https:')).toMatchObject({
      path: '/',
      sameSite: 'Strict',
      secure: true,
      expires: 14
    })
  })

  test('setSessionToken stores the token with the shared attributes', () => {
    setSessionToken('token-value', 'https:')

    expect(mockedCookies.set).toHaveBeenCalledTimes(1)
    expect(mockedCookies.set).toHaveBeenCalledWith('jwt', 'token-value', {
      path: '/',
      sameSite: 'Strict',
      secure: true,
      expires: 14
    })
  })

  test('clearSessionToken removes the cookie with the matching path', () => {
    clearSessionToken()

    expect(mockedCookies.remove).toHaveBeenCalledTimes(1)
    expect(mockedCookies.remove).toHaveBeenCalledWith('jwt', { path: '/' })
  })

  test('set then clear targets the same cookie name and path', () => {
    setSessionToken('token-value', 'https:')
    clearSessionToken('https:')

    const setCall = mockedCookies.set.mock.calls[0]
    const removeCall = mockedCookies.remove.mock.calls[0]
    expect(setCall?.[0]).toBe('jwt')
    expect(removeCall?.[0]).toBe('jwt')
    expect(removeCall?.[1].path).toBe(setCall?.[2].path)
  })

  test('plain http resolves secure:false so the browser keeps the cookie', () => {
    expect(resolveSessionCookieAttributes('http:')).toMatchObject({
      path: '/',
      sameSite: 'Strict',
      secure: false,
      expires: 14
    })
  })

  test('setSessionToken over plain http omits the secure flag', () => {
    setSessionToken('token-value', 'http:')

    expect(mockedCookies.set).toHaveBeenCalledTimes(1)
    expect(mockedCookies.set).toHaveBeenCalledWith('jwt', 'token-value', {
      path: '/',
      sameSite: 'Strict',
      secure: false,
      expires: 14
    })
  })

  test('clearSessionToken targets the same path the set used, on both protocols', () => {
    for (const protocol of ['http:', 'https:']) {
      vi.clearAllMocks()
      setSessionToken('token-value', protocol)
      clearSessionToken(protocol)

      const setCall = mockedCookies.set.mock.calls[0] as unknown[] | undefined
      const removeCall = mockedCookies.remove.mock.calls[0] as unknown[] | undefined
      expect(setCall?.[0]).toBe('jwt')
      expect(removeCall?.[0]).toBe('jwt')
      expect((removeCall?.[1] as { path: string }).path).toBe(
        (setCall?.[2] as { path: string }).path
      )
    }
  })
})
