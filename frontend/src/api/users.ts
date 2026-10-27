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

/// Register a standard user via `POST /post/users/register`. The backend
/// accepts no `admin` field (`deny_unknown_fields`), so only `{ userId,
/// password }` is sent; elevation is `PUT /put/users/admin` only.
export const createUser = async (
  userId: string,
  password: string,
  invite?: string
): Promise<UserSummary> => {
  const body: { userId: string; password: string; invite?: string } = {
    userId,
    password
  }
  // Include the invite token only when non-empty: an empty string counts as
  // absent on the backend (see RegisterRequest.invite).
  if (invite !== undefined && invite !== '') {
    body.invite = invite
  }
  const response = await axios.post<UserSummary>('/post/users/register', body)
  return response.data
}

/// Enroll the first administrator via `POST /post/users/bootstrap`.
/// Open only while the user store is empty and `--bootstrap-admin` is set;
/// the backend forces `admin: true`. Named after the route (cf. `createUser`
/// → register) so the api layer stays route-derived.
export const bootstrapUser = async (userId: string, password: string): Promise<UserSummary> => {
  const response = await axios.post<UserSummary>('/post/users/bootstrap', {
    userId,
    password
  })
  return response.data
}

export const setUserAdmin = async (userId: string, admin: boolean): Promise<void> => {
  await axios.put('/put/users/admin', {
    userId,
    admin
  })
}

export const deleteUser = async (userId: string): Promise<void> => {
  await axios.delete(`/delete/users/${encodeURIComponent(userId)}`)
}

/// Change a user's password via `PUT /put/users/password`. A self-change
/// always presents the current password; only an admin recovering another
/// user omits it (no UI for that path yet).
export const setUserPassword = async (
  userId: string,
  oldPassword: string,
  newPassword: string
): Promise<void> => {
  await axios.put(
    '/put/users/password',
    {
      userId,
      oldPassword,
      newPassword
    },
    {
      headers: { 'X-Picasu-Password': oldPassword }
    }
  )
}

/// Sign in with `{ userId, password }` and return the session profile.
///
/// The JWT travels in an HttpOnly `Set-Cookie` header the page JS never
/// sees; the body carries only `{ userId, admin }` for the client to
/// keep in the session store. Rejects with the backend error unchanged
/// (401 on unknown user or wrong password); there is no legacy fallback
/// path.
export const login = async (userId: string, password: string): Promise<UserSummary> => {
  const response = await axios.post('/post/users/authenticate', { userId, password })
  return z.object({ userId: z.string(), admin: z.boolean() }).parse(response.data)
}

/// Sign out: clears the HttpOnly session cookie server-side (page JS
/// cannot clear HttpOnly cookies itself). Idempotent — succeeds with or
/// without a live session. Callers still clear local state unconditionally
/// so logout sticks even when the round trip fails.
export const logout = async (): Promise<void> => {
  await axios.post('/post/logout')
}
