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
export const createUser = async (userId: string, password: string): Promise<UserSummary> => {
  const response = await axios.post<UserSummary>('/post/users/register', {
    userId,
    password
  })
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

/// Sign in with `{ userId, password }` and return the JWT on success.
/// Rejects with the backend error unchanged (401 on unknown user or wrong
/// password); there is no legacy fallback path.
export const login = async (userId: string, password: string): Promise<string> => {
  const response = await axios.post('/post/authenticate', { userId, password })
  return z.string().parse(response.data)
}
