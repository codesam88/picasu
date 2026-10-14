import Cookies from 'js-cookie'

export const SESSION_COOKIE_NAME = 'jwt'

/** Single source of truth for the session cookie attributes. */
export const SESSION_COOKIE_ATTRIBUTES = {
  path: '/',
  sameSite: 'Strict',
  secure: true,
  expires: 14
} as const

export const setSessionToken = (token: string): void => {
  Cookies.set(SESSION_COOKIE_NAME, token, { ...SESSION_COOKIE_ATTRIBUTES })
}

export const clearSessionToken = (): void => {
  Cookies.remove(SESSION_COOKIE_NAME, { path: SESSION_COOKIE_ATTRIBUTES.path })
}
