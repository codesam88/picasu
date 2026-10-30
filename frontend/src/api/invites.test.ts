import { describe, expect, test, vi, beforeEach } from 'vitest'
import axios from 'axios'
import { buildInviteLink, createInvite, getInviteLimits, InviteViewSchema } from './invites'

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

describe('getInviteLimits', () => {
  test('returns maxExpiryHours from GET /get/invites/limits', async () => {
    mockedAxios.get = vi.fn().mockResolvedValue({ data: { maxExpiryHours: 8760 } })
    await expect(getInviteLimits()).resolves.toEqual({ maxExpiryHours: 8760 })
    expect(mockedAxios.get).toHaveBeenCalledWith('/get/invites/limits')
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

describe('buildInviteLink', () => {
  test('appends the bound username so register can prefill it', () => {
    expect(buildInviteLink('https://host', 'tok', 'maria')).toBe(
      'https://host/register?invite=tok&username=maria'
    )
  })

  test('omits username for open invites', () => {
    expect(buildInviteLink('https://host', 'tok')).toBe('https://host/register?invite=tok')
    expect(buildInviteLink('https://host', 'tok', '   ')).toBe('https://host/register?invite=tok')
  })
})
