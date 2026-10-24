import { describe, expect, test, vi, beforeEach } from 'vitest'
import axios from 'axios'
import {
  listUsers,
  createUser,
  bootstrapUser,
  login,
  logout,
  setUserAdmin,
  setUserPassword
} from './users'

vi.mock('axios')

const mockedAxios = axios as unknown as {
  get: ReturnType<typeof vi.fn>
  put: ReturnType<typeof vi.fn>
  post: ReturnType<typeof vi.fn>
}

beforeEach(() => {
  vi.resetAllMocks()
})

describe('listUsers', () => {
  test('returns the user list from GET /get/users', async () => {
    mockedAxios.get = vi.fn().mockResolvedValue({
      data: [
        { userId: 'bob', admin: false },
        { userId: 'root', admin: true }
      ]
    })
    await expect(listUsers()).resolves.toEqual([
      { userId: 'bob', admin: false },
      { userId: 'root', admin: true }
    ])
    expect(mockedAxios.get).toHaveBeenCalledWith('/get/users')
  })
})

describe('setUserAdmin', () => {
  test('puts the admin flag to /put/users/admin', async () => {
    mockedAxios.put = vi.fn().mockResolvedValue({ data: '' })
    await setUserAdmin('bob', true)
    expect(mockedAxios.put).toHaveBeenCalledWith('/put/users/admin', {
      userId: 'bob',
      admin: true
    })
  })
})

describe('createUser', () => {
  test('posts userId and password without admin to /post/users/register', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({ data: { userId: 'bob', admin: false } })
    await expect(createUser('bob', 'secret-pw')).resolves.toEqual({
      userId: 'bob',
      admin: false
    })
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/users/register', {
      userId: 'bob',
      password: 'secret-pw'
    })
  })

  test('includes invite in the body when provided and non-empty', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({ data: { userId: 'bob', admin: false } })
    await createUser('bob', 'secret-pw', 'tok-123')
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/users/register', {
      userId: 'bob',
      password: 'secret-pw',
      invite: 'tok-123'
    })
  })

  test('omits invite from the body when empty or missing', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({ data: { userId: 'bob', admin: false } })
    await createUser('bob', 'secret-pw', '')
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/users/register', {
      userId: 'bob',
      password: 'secret-pw'
    })
  })
})

describe('bootstrapUser', () => {
  test('posts userId and password without admin to /post/users/bootstrap', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({ data: { userId: 'root', admin: true } })
    await expect(bootstrapUser('root', 'secret-pw')).resolves.toEqual({
      userId: 'root',
      admin: true
    })
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/users/bootstrap', {
      userId: 'root',
      password: 'secret-pw'
    })
  })
})

describe('login', () => {
  const axios401 = () => {
    const err = new Error('Request failed with status code 401') as Error & {
      response: { status: number }
    }
    err.response = { status: 401 }
    return err
  }

  test('object-success costs one POST and returns the session profile, never a token', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({ data: { userId: 'bob', admin: false } })
    await expect(login('bob', 'secret')).resolves.toEqual({ userId: 'bob', admin: false })
    expect(mockedAxios.post).toHaveBeenCalledTimes(1)
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/users/authenticate', {
      userId: 'bob',
      password: 'secret'
    })
  })

  test('object-401 rejects the same error with exactly one POST (no migration retry)', async () => {
    const first = axios401()
    mockedAxios.post = vi.fn().mockRejectedValueOnce(first)
    await expect(login('admin', 'wrong')).rejects.toBe(first)
    expect(mockedAxios.post).toHaveBeenCalledTimes(1)
  })

  test('network error rethrows as-is with exactly one POST', async () => {
    const err = new Error('Network Error') as Error & { response?: unknown }
    mockedAxios.post = vi.fn().mockRejectedValueOnce(err)
    await expect(login('bob', 'secret')).rejects.toBe(err)
    expect(mockedAxios.post).toHaveBeenCalledTimes(1)
  })
})

describe('setUserPassword', () => {
  test('puts userId, old and new passwords to /put/users/password', async () => {
    mockedAxios.put = vi.fn().mockResolvedValue({ data: '' })
    await setUserPassword('alice', 'old-secret', 'new-secret')
    expect(mockedAxios.put).toHaveBeenCalledWith('/put/users/password', {
      userId: 'alice',
      oldPassword: 'old-secret',
      newPassword: 'new-secret'
    })
  })
})

describe('logout', () => {
  test('posts to /post/logout with no body', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({ data: '' })
    await expect(logout()).resolves.toBeUndefined()
    expect(mockedAxios.post).toHaveBeenCalledTimes(1)
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/logout')
  })
})
