import { describe, expect, test, vi, beforeEach } from 'vitest'
import axios from 'axios'
import { resetAuthKey } from './config'

vi.mock('axios')

const mockedAxios = axios as unknown as {
  get: ReturnType<typeof vi.fn>
  put: ReturnType<typeof vi.fn>
  post: ReturnType<typeof vi.fn>
}

beforeEach(() => {
  vi.resetAllMocks()
})

describe('resetAuthKey', () => {
  test('posts to the reset route and returns the key state', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({ data: { hasAuthKey: true } })
    await expect(resetAuthKey()).resolves.toEqual({ hasAuthKey: true })
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/config/auth-key/reset')
  })
})
