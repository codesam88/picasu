import { describe, expect, test, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import {
  canSeeUserPanel,
  errorToMessage,
  selfRowState,
  shouldLoadAppConfig,
  useCurrentUser
} from './currentUser'
import { useSessionStore } from '@/store/sessionStore'

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('useCurrentUser', () => {
  test('is null with no session profile', () => {
    expect(useCurrentUser().value).toBeNull()
  })

  test('maps an admin profile to an admin user', () => {
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: true })
    expect(useCurrentUser().value).toEqual({ id: 'alice', isAdmin: true })
  })

  test('maps a non-admin profile to a non-admin user', () => {
    useSessionStore('mainId').setProfile({ userId: 'bob', admin: false })
    expect(useCurrentUser().value).toEqual({ id: 'bob', isAdmin: false })
  })

  test('goes null again when the profile is cleared', () => {
    const session = useSessionStore('mainId')
    session.setProfile({ userId: 'alice', admin: true })
    session.clearProfile()
    expect(useCurrentUser().value).toBeNull()
  })
})

describe('canSeeUserPanel', () => {
  test('only admins see the panel', () => {
    expect(canSeeUserPanel({ id: 'a', isAdmin: true })).toBe(true)
    expect(canSeeUserPanel({ id: 'b', isAdmin: false })).toBe(false)
    expect(canSeeUserPanel(null)).toBe(false)
  })
})

describe('selfRowState', () => {
  test('self row is locked only when sole admin', () => {
    expect(selfRowState({ id: 'a', isAdmin: true }, 'a', 1)).toBe('sole-admin')
    expect(selfRowState({ id: 'a', isAdmin: true }, 'a', 2)).toBe('ok')
  })

  test('other rows are always actionable', () => {
    expect(selfRowState({ id: 'a', isAdmin: true }, 'b', 1)).toBe('ok')
    expect(selfRowState(null, 'b', 1)).toBe('ok')
  })
})

describe('errorToMessage', () => {
  test('401 signals a login redirect', () => {
    expect(errorToMessage(401)).toEqual({ redirectLogin: true })
  })

  test('409/404/400 surface the server message verbatim', () => {
    expect(errorToMessage(409, 'Cannot demote the last admin')).toEqual({
      redirectLogin: false,
      message: 'Cannot demote the last admin'
    })
    expect(errorToMessage(404, 'User not found')).toEqual({
      redirectLogin: false,
      message: 'User not found'
    })
    expect(errorToMessage(400, 'Invalid user id')).toEqual({
      redirectLogin: false,
      message: 'Invalid user id'
    })
  })

  test('falls back to a generic message when the server sent none', () => {
    const result = errorToMessage(500)
    expect(result.redirectLogin).toBe(false)
    if (!result.redirectLogin) expect(result.message.length).toBeGreaterThan(0)
  })
})

describe('shouldLoadAppConfig', () => {
  test('non-admin sessions skip the admin-only config endpoint', () => {
    expect(shouldLoadAppConfig({ id: 'bob', isAdmin: false })).toBe(false)
  })

  test('admins, share visitors and logged-out boot still load config', () => {
    expect(shouldLoadAppConfig({ id: 'alice', isAdmin: true })).toBe(true)
    expect(shouldLoadAppConfig(null)).toBe(true)
  })
})
