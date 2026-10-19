import { describe, expect, test, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { SESSION_PROFILE_KEY, useSessionStore } from './sessionStore'

const store = () => {
  const backing = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => backing.get(key) ?? null,
    setItem: (key: string, value: string) => {
      backing.set(key, value)
    },
    removeItem: (key: string) => {
      backing.delete(key)
    }
  })
  return backing
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.unstubAllGlobals()
})

describe('useSessionStore', () => {
  test('starts with no profile when storage is empty', () => {
    store()
    expect(useSessionStore('mainId').profile).toBeNull()
  })

  test('setProfile persists the profile for reloads', () => {
    const backing = store()
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: true })

    expect(JSON.parse(backing.get(SESSION_PROFILE_KEY) ?? '')).toEqual({
      userId: 'alice',
      admin: true
    })
  })

  test('a fresh store hydrates the persisted profile (reload)', () => {
    const backing = store()
    backing.set(SESSION_PROFILE_KEY, JSON.stringify({ userId: 'alice', admin: true }))
    setActivePinia(createPinia())

    expect(useSessionStore('mainId').profile).toEqual({ userId: 'alice', admin: true })
  })

  test('clearProfile drops both memory and storage', () => {
    const backing = store()
    const session = useSessionStore('mainId')
    session.setProfile({ userId: 'alice', admin: true })
    session.clearProfile()

    expect(session.profile).toBeNull()
    expect(backing.get(SESSION_PROFILE_KEY)).toBeUndefined()
  })

  test('malformed storage resolves to no profile and self-cleans', () => {
    const backing = store()
    backing.set(SESSION_PROFILE_KEY, '{"userId":42}')
    setActivePinia(createPinia())

    expect(useSessionStore('mainId').profile).toBeNull()
    expect(backing.get(SESSION_PROFILE_KEY)).toBeUndefined()
  })

  test('works with no localStorage at all (node-env safety)', () => {
    expect(useSessionStore('mainId').profile).toBeNull()
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: false })
    expect(useSessionStore('mainId').profile).toEqual({ userId: 'alice', admin: false })
    useSessionStore('mainId').clearProfile()
    expect(useSessionStore('mainId').profile).toBeNull()
  })
})
