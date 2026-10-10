import { describe, expect, test, vi, beforeEach } from 'vitest'
import axios from 'axios'
import { listUsers, login, setUserAdmin } from './users'

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

describe('login', () => {
  const axios401 = () => {
    const err = new Error('Request failed with status code 401') as Error & {
      response: { status: number }
    }
    err.response = { status: 401 }
    return err
  }

  test('object-success costs one POST and returns the token', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({ data: 'jwt-token' })
    await expect(login('bob', 'secret')).resolves.toBe('jwt-token')
    expect(mockedAxios.post).toHaveBeenCalledTimes(1)
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/authenticate', {
      userId: 'bob',
      password: 'secret'
    })
  })

  test('object-401 then string-401 rethrows the first error with exactly two POSTs', async () => {
    const first = axios401()
    const second = axios401()
    mockedAxios.post = vi.fn().mockRejectedValueOnce(first).mockRejectedValueOnce(second)
    await expect(login('admin', 'wrong')).rejects.toBe(first)
    expect(mockedAxios.post).toHaveBeenCalledTimes(2)
    expect(mockedAxios.post).toHaveBeenNthCalledWith(2, '/post/authenticate', 'wrong')
  })

  test('object-401 then string-ok then object-ok returns the token with three POSTs', async () => {
    mockedAxios.post = vi
      .fn()
      .mockRejectedValueOnce(axios401())
      .mockResolvedValueOnce({ data: 'migrated-token' })
      .mockResolvedValueOnce({ data: 'jwt-token' })
    await expect(login('admin', 'legacy-secret')).resolves.toBe('jwt-token')
    expect(mockedAxios.post).toHaveBeenCalledTimes(3)
    expect(mockedAxios.post).toHaveBeenNthCalledWith(1, '/post/authenticate', {
      userId: 'admin',
      password: 'legacy-secret'
    })
    expect(mockedAxios.post).toHaveBeenNthCalledWith(2, '/post/authenticate', 'legacy-secret')
    expect(mockedAxios.post).toHaveBeenNthCalledWith(3, '/post/authenticate', {
      userId: 'admin',
      password: 'legacy-secret'
    })
  })

  test('non-401 error rethrows without any fallback POST', async () => {
    const err = new Error('Network Error') as Error & { response?: unknown }
    mockedAxios.post = vi.fn().mockRejectedValueOnce(err)
    await expect(login('bob', 'secret')).rejects.toBe(err)
    expect(mockedAxios.post).toHaveBeenCalledTimes(1)
  })
})
