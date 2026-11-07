/// Theme scoping in constStore: loads and saves under the current session
/// identity (global key when logged out) and falls back to the app default
/// when nothing is stored. The settings layer is mocked — key-scheme
/// behavior lives in settingsDb.test.ts.
import { beforeEach, describe, expect, test, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { getTheme, storeTheme } from '@/db/settingsDb'
import { useSessionStore } from '@/store/sessionStore'
import { useConstStore } from './constStore'

vi.mock('@/db/settingsDb', () => ({
  storeSubRowHeightScale: vi.fn(),
  getSubRowHeightScale: vi.fn(),
  storeShowInfo: vi.fn(),
  getShowInfo: vi.fn(),
  storeConcurrencyNumber: vi.fn(),
  getConcurrencyNumber: vi.fn(),
  storeLimitRation: vi.fn(),
  getLimitRation: vi.fn(),
  storeTheme: vi.fn(),
  getTheme: vi.fn(),
  storeShowFilenameChip: vi.fn(),
  getShowFilenameChip: vi.fn(),
  storeDisableImg: vi.fn(),
  getDisableImg: vi.fn()
}))

beforeEach(() => {
  vi.unstubAllGlobals()
  vi.resetAllMocks()
  // sessionStore hydrates its profile from localStorage at store creation.
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
  setActivePinia(createPinia())
})

const constStore = () => useConstStore('mainId')

describe('theme loading', () => {
  test('loadThemeFor loads the per-user key', async () => {
    vi.mocked(getTheme).mockResolvedValue('light')

    await constStore().loadThemeFor('alice')

    expect(getTheme).toHaveBeenCalledWith('alice')
    expect(constStore().theme).toBe('light')
  })

  test('loadThemeFor(null) reads the global key', async () => {
    vi.mocked(getTheme).mockResolvedValue('dark')

    await constStore().loadThemeFor(null)

    expect(getTheme).toHaveBeenCalledWith(undefined)
    expect(constStore().theme).toBe('dark')
  })

  test('loadThemeFor falls back to the app default when nothing is stored', async () => {
    vi.mocked(getTheme).mockResolvedValue(null)
    constStore().theme = 'light'

    await constStore().loadThemeFor('bob')

    expect(constStore().theme).toBe('dark')
  })

  test('loadTheme resolves the persisted session identity', async () => {
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: false })
    vi.mocked(getTheme).mockResolvedValue('light')

    await constStore().loadTheme()

    expect(getTheme).toHaveBeenCalledWith('alice')
    expect(constStore().theme).toBe('light')
  })

  test('loadTheme falls back to the global key when logged out', async () => {
    vi.mocked(getTheme).mockResolvedValue('dark')

    await constStore().loadTheme()

    expect(getTheme).toHaveBeenCalledWith(undefined)
    expect(constStore().theme).toBe('dark')
  })
})

describe('theme saving', () => {
  test('updateTheme saves under the current user key', async () => {
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: false })

    await constStore().updateTheme('light')

    expect(storeTheme).toHaveBeenCalledWith('light', 'alice')
    expect(constStore().theme).toBe('light')
  })

  test('updateTheme saves the global key when logged out', async () => {
    await constStore().updateTheme('light')

    expect(storeTheme).toHaveBeenCalledWith('light', undefined)
    expect(constStore().theme).toBe('light')
  })

  test('toggleTheme saves under the current user key and applies to the Vuetify handle', async () => {
    useSessionStore('mainId').setProfile({ userId: 'alice', admin: false })
    constStore().theme = 'dark'
    const handle = { global: { name: { value: 'dark' } } }

    await constStore().toggleTheme(handle)

    expect(storeTheme).toHaveBeenCalledWith('light', 'alice')
    expect(handle.global.name.value).toBe('light')
  })

  test('toggleTheme saves the global key when logged out', async () => {
    constStore().theme = 'dark'

    await constStore().toggleTheme()

    expect(storeTheme).toHaveBeenCalledWith('light', undefined)
  })
})
