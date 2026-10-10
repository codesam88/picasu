import axios from 'axios'
import { z } from 'zod'

export interface UserSummary {
  userId: string
  admin: boolean
}

export const listUsers = async (): Promise<UserSummary[]> => {
  const response = await axios.get<UserSummary[]>('/get/users')
  return response.data
}

export const setUserAdmin = async (userId: string, admin: boolean): Promise<void> => {
  await axios.put('/put/users/admin', {
    userId,
    admin
  })
}

/// Sign in with the object login path, falling back to the bare-string
/// legacy-migration path on 401 only.
///
/// A pre-migration deployment (empty user store + legacy config password)
/// answers the object path with 401 (unknown user), so the first post-upgrade
/// login retries once as the bare-string `password`, which bootstraps the
/// claimed id (or `admin` for the string path) as the first admin, then
/// retries the object path to mint the token.
///
/// Cost: one-time migration costs two KDF rounds on the first post-upgrade
/// login only; steady-state migrated logins cost exactly one request; failed
/// logins on migrated systems cost two cheap 401s — the string path fails fast
/// without hashing (it checks `user_count` before any password work; see
/// `ensure_migrated_from_legacy` in backend/src/router/post/authenticate.rs).
/// If the string POST also 401s, the FIRST error is rethrown so wrong-password
/// UX on migrated systems is unchanged. Any non-401 error rethrows immediately
/// without a fallback POST.
export const login = async (userId: string, password: string): Promise<string> => {
  const postObject = async (): Promise<string> => {
    const response = await axios.post('/post/authenticate', { userId, password })
    return z.string().parse(response.data)
  }
  try {
    return await postObject()
  } catch (err: unknown) {
    if ((err as { response?: { status?: number } })?.response?.status !== 401) {
      throw err
    }
    try {
      await axios.post('/post/authenticate', password)
    } catch (stringErr: unknown) {
      if ((stringErr as { response?: { status?: number } })?.response?.status === 401) {
        throw err
      }
      throw stringErr
    }
    return await postObject()
  }
}
