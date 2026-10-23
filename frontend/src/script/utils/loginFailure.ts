import axios from 'axios'

/**
 * Inline-banner text for a failed login attempt, or null when this error
 * is owned elsewhere. A 401 (unknown user or wrong password) belongs to
 * the login page itself — the interceptor stays silent for
 * `/post/users/authenticate` — so the page renders it inline next to the
 * 429 countdown. The copy is fixed client-side (not the server message)
 * so the wording stays stable regardless of backend phrasing. Anything
 * else (429, 5xx, network) keeps its existing path: the countdown, a
 * rethrow to the interceptor, or the generic handler.
 */
export const loginFailureMessage = (error: unknown): string | null => {
  if (!axios.isAxiosError(error) || error.response?.status !== 401) return null
  return 'Invalid username or password'
}
