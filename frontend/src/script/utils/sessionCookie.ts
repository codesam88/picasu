import Cookies from 'js-cookie'

export const SESSION_COOKIE_NAME = 'jwt'

export const SESSION_COOKIE_PATH = '/'

/// Resolve the session cookie attributes for `protocol`. The `secure`
/// flag is set only on https: a hardcoded `secure: true` makes browsers
/// reject the cookie over plain-HTTP deployments (the default), so every
/// page 401s back to login. Defaults to the page protocol in browsers and
/// to plain http elsewhere, so node-env unit tests cover each branch by
/// passing the protocol explicitly.
export const resolveSessionCookieAttributes = (protocol: string = getPageProtocol()) => ({
  path: SESSION_COOKIE_PATH,
  sameSite: 'Strict' as const,
  secure: protocol === 'https:',
  expires: 14
})

function getPageProtocol(): string {
  if (typeof window !== 'undefined' && typeof window.location?.protocol === 'string') {
    return window.location.protocol
  }
  return 'http:'
}

export const setSessionToken = (token: string, protocol?: string): void => {
  Cookies.set(
    SESSION_COOKIE_NAME,
    token,
    // Spread so the attributes stay a plain js-cookie options object.
    { ...resolveSessionCookieAttributes(protocol ?? getPageProtocol()) }
  )
}

export const clearSessionToken = (protocol?: string): void => {
  // Removal must target the same cookie the set created: resolve the path
  // through the same attributes so the two cannot drift apart.
  const { path } = resolveSessionCookieAttributes(protocol ?? getPageProtocol())
  Cookies.remove(SESSION_COOKIE_NAME, { path })
}
