import { describe, expect, test, vi, beforeEach } from 'vitest'
import axios from 'axios'
import { getSelfProfile } from './profile'

vi.mock('axios')

const mockedAxios = axios as unknown as {
  get: ReturnType<typeof vi.fn>
}

beforeEach(() => {
  vi.resetAllMocks()
})

describe('getSelfProfile', () => {
  test('GETs /get/self/profile and parses', async () => {
    mockedAxios.get = vi.fn().mockResolvedValue({
      data: {
        user: { userId: 'bob', admin: false, home: null },
        settings: {}
      }
    })
    const result = await getSelfProfile()
    expect(result).toEqual({
      user: { userId: 'bob', admin: false, home: null },
      settings: {}
    })
    expect(mockedAxios.get).toHaveBeenCalledWith('/get/self/profile')
  })
})
