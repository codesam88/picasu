import { describe, expect, test, vi, beforeEach } from 'vitest'
import axios from 'axios'
import { getServerInfo } from './serverInfo'

vi.mock('axios')

const mockedAxios = axios as unknown as {
  get: ReturnType<typeof vi.fn>
}

beforeEach(() => {
  vi.resetAllMocks()
})

describe('getServerInfo', () => {
  test('parses the anonymous server-info shape from GET /get/server-info', async () => {
    mockedAxios.get = vi
      .fn()
      .mockResolvedValue({ data: { enrolled: false, enrollmentOpen: true, captcha: null } })
    await expect(getServerInfo()).resolves.toEqual({
      enrolled: false,
      enrollmentOpen: true,
      captcha: null
    })
    expect(mockedAxios.get).toHaveBeenCalledWith('/get/server-info')
    expect(mockedAxios.get).toHaveBeenCalledTimes(1)
  })

  test('rejects a shape missing a key', async () => {
    mockedAxios.get = vi.fn().mockResolvedValue({ data: { enrolled: false, captcha: null } })
    await expect(getServerInfo()).rejects.toThrow()
  })

  test('rejects a shape with a wrong type', async () => {
    mockedAxios.get = vi
      .fn()
      .mockResolvedValue({ data: { enrolled: 'no', enrollmentOpen: true, captcha: null } })
    await expect(getServerInfo()).rejects.toThrow()
  })
})
