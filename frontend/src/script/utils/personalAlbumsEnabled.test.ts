import { describe, expect, test, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { usePersonalAlbumsEnabled } from './personalAlbumsEnabled'
import { useSessionStore } from '@/store/sessionStore'
import { useConfigStore } from '@/store/configStore'

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('usePersonalAlbumsEnabled', () => {
  test('true when authenticated and config flag true', () => {
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: false })
    useConfigStore('mainId').config = { personalAlbumsEnabled: true } as any
    expect(usePersonalAlbumsEnabled().value).toBe(true)
  })

  test('false when config flag false', () => {
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: false })
    useConfigStore('mainId').config = { personalAlbumsEnabled: false } as any
    expect(usePersonalAlbumsEnabled().value).toBe(false)
  })

  test('false when config flag missing', () => {
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: false })
    useConfigStore('mainId').config = {} as any
    expect(usePersonalAlbumsEnabled().value).toBe(false)
  })

  test('false when logged out', () => {
    useConfigStore('mainId').config = { personalAlbumsEnabled: true } as any
    expect(usePersonalAlbumsEnabled().value).toBe(false)
  })
})
