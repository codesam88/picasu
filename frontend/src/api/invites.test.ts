import { describe, expect, test, vi, beforeEach } from 'vitest'
import axios from 'axios'
import { createInvite, InviteViewSchema } from './invites'

vi.mock('axios')

const mockedAxios = axios as unknown as {
  get: ReturnType<typeof vi.fn>
  put: ReturnType<typeof vi.fn>
  post: ReturnType<typeof vi.fn>
  delete: ReturnType<typeof vi.fn>
}

beforeEach(() => {
  vi.resetAllMocks()
})

describe('createInvite', () => {
  test('includes username when provided and non-empty', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({
      data: {
        token: 'tok',
        invite: {
          id: 'id',
          prefix: 'pref',
          createdBy: 'admin',
          expiresAt: 123,
          usedBy: null,
          username: 'alice'
        }
      }
    })
    await createInvite(24, 'alice')
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/invites', {
      expiresInHours: 24,
      username: 'alice'
    })
  })

  test('omits username when missing or empty', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({
      data: {
        token: 'tok',
        invite: {
          id: 'id',
          prefix: 'pref',
          createdBy: 'admin',
          expiresAt: 123,
          usedBy: null,
          username: null
        }
      }
    })
    await createInvite(168)
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/invites', {
      expiresInHours: 168
    })

    mockedAxios.post = vi.fn().mockResolvedValue({
      data: {
        token: 'tok',
        invite: {
          id: 'id',
          prefix: 'pref',
          createdBy: 'admin',
          expiresAt: 123,
          usedBy: null,
          username: null
        }
      }
    })
    await createInvite(168, '')
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/invites', {
      expiresInHours: 168
    })
  })
})

describe('InviteViewSchema', () => {
  test('parses username as nullable string', () => {
    const parsed = InviteViewSchema.parse({
      id: 'id',
      prefix: 'pref',
      createdBy: 'admin',
      expiresAt: null,
      usedBy: null,
      username: null
    })
    expect(parsed.username).toBeNull()
  })
})
