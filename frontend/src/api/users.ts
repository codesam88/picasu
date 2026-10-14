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

/// Create a user record. On an empty store with `--bootstrap-admin` the
/// backend forces `admin: true` regardless of the body's flag, so the
/// first-user enrollment form always sends `admin: true`.
export const createUser = async (userId: string, password: string): Promise<UserSummary> => {
  const response = await axios.post<UserSummary>('/post/users/create', {
    userId,
    password,
    admin: true
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
