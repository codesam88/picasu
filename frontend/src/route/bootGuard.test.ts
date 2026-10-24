import { describe, expect, test, vi } from 'vitest'

import type { ServerInfo } from '@/api/serverInfo'
import { decideBootRoute } from './bootGuard'

function okDeps(overrides?: {
  isShareTarget?: boolean
  profilePresent?: boolean
  toName?: string | undefined
  hasInviteToken?: boolean
  serverInfo?: () => Promise<ServerInfo>
}) {
  return {
    isShareTarget: false,
    profilePresent: false,
    toName: 'timeline' as string | undefined,
    hasInviteToken: false,
    serverInfo: vi.fn(async (): Promise<ServerInfo> => ({
      enrolled: true,
      enrollmentOpen: false,
      captcha: null
    })),
    ...overrides
  }
}

describe('decideBootRoute', () => {
  test('share target allows immediately without a probe', async () => {
    const deps = okDeps({ isShareTarget: true })

    await expect(decideBootRoute(deps)).resolves.toBe(true)
    expect(deps.serverInfo).not.toHaveBeenCalled()
  })

  test('profile present allows immediately without a probe', async () => {
    const deps = okDeps({ profilePresent: true })

    await expect(decideBootRoute(deps)).resolves.toBe(true)
    expect(deps.serverInfo).not.toHaveBeenCalled()
  })

  test('needs-enrollment redirects to register unless already there', async () => {
    const serverInfo = vi.fn(async (): Promise<ServerInfo> => ({
      enrolled: false,
      enrollmentOpen: true,
      captcha: null
    }))

    await expect(decideBootRoute(okDeps({ toName: 'timeline', serverInfo }))).resolves.toEqual({
      name: 'register'
    })
    await expect(decideBootRoute(okDeps({ toName: 'register', serverInfo }))).resolves.toBe(true)
    expect(serverInfo).toHaveBeenCalledTimes(2)
  })

  test('enrolled store with no profile redirects to login unless already there', async () => {
    const serverInfo = vi.fn(async (): Promise<ServerInfo> => ({
      enrolled: true,
      enrollmentOpen: false,
      captcha: null
    }))

    await expect(decideBootRoute(okDeps({ toName: 'login', serverInfo }))).resolves.toBe(true)
    await expect(decideBootRoute(okDeps({ toName: 'timeline', serverInfo }))).resolves.toEqual({
      name: 'login'
    })
    expect(serverInfo).toHaveBeenCalledTimes(2)
  })

  test('closed install with no profile falls through to login, never register', async () => {
    const serverInfo = vi.fn(async (): Promise<ServerInfo> => ({
      enrolled: false,
      enrollmentOpen: false,
      captcha: null
    }))

    await expect(decideBootRoute(okDeps({ toName: 'timeline', serverInfo }))).resolves.toEqual({
      name: 'login'
    })
  })

  test('invite token on register renders even on an enrolled store', async () => {
    const serverInfo = vi.fn(async (): Promise<ServerInfo> => ({
      enrolled: true,
      enrollmentOpen: false,
      captcha: null
    }))

    await expect(
      decideBootRoute(okDeps({ toName: 'register', serverInfo, hasInviteToken: true }))
    ).resolves.toBe(true)
  })

  test('no invite token on register still redirects to login on an enrolled store', async () => {
    const serverInfo = vi.fn(async (): Promise<ServerInfo> => ({
      enrolled: true,
      enrollmentOpen: false,
      captcha: null
    }))

    await expect(
      decideBootRoute(okDeps({ toName: 'register', serverInfo, hasInviteToken: false }))
    ).resolves.toEqual({ name: 'login' })
  })

  test('rejected server-info ping degrades to the login fall-through without aborting', async () => {
    const serverInfo = vi.fn(async (): Promise<ServerInfo> => {
      throw new Error('ping failed')
    })

    await expect(decideBootRoute(okDeps({ toName: 'timeline', serverInfo }))).resolves.toEqual({
      name: 'login'
    })
    await expect(decideBootRoute(okDeps({ toName: 'login', serverInfo }))).resolves.toBe(true)
    expect(serverInfo).toHaveBeenCalledTimes(2)
  })
})
