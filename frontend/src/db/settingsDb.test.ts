/// Theme key scoping in the uiSettings store: `theme` is the logged-out
/// global default, `theme:<userId>` is per-user, and reads fall back to the
/// global key so pre-scoping values keep working.
import { beforeEach, describe, expect, test, vi } from 'vitest'
import { deleteTheme, getTheme, storeTheme } from './settingsDb'

interface FakeRequest {
  result?: unknown
  onupgradeneeded?: (event: { target: FakeRequest }) => void
  onsuccess?: (event: { target: FakeRequest }) => void
  onerror?: (event: { target: FakeRequest }) => void
}

/// Node has no IndexedDB. This stand-in covers exactly what settingsDb
/// uses — open (+upgrade), transaction/objectStore, get/put/delete — with
/// every request completing on a later microtask like the real API.
/// Returns the backing map so tests can seed keys directly.
function stubIndexedDB(): Map<string, unknown> {
  const data = new Map<string, unknown>()
  const stores = new Set<string>()

  const requestResult = (run: () => unknown): FakeRequest => {
    const request: FakeRequest = {}
    queueMicrotask(() => {
      request.result = run()
      request.onsuccess?.({ target: request })
    })
    return request
  }

  const db = {
    objectStoreNames: { contains: (name: string) => stores.has(name) },
    createObjectStore: (name: string) => {
      stores.add(name)
    },
    transaction: () => ({
      objectStore: () => ({
        get: (key: string) => requestResult(() => data.get(key)),
        put: (value: unknown, key: string) =>
          requestResult(() => {
            data.set(key, value)
            return value
          }),
        delete: (key: string) =>
          requestResult(() => {
            data.delete(key)
            return undefined
          })
      })
    })
  }

  vi.stubGlobal('indexedDB', {
    open: () => {
      const request: FakeRequest = {}
      queueMicrotask(() => {
        request.result = db
        request.onupgradeneeded?.({ target: request })
        request.onsuccess?.({ target: request })
      })
      return request
    }
  })

  return data
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

describe('theme key scoping', () => {
  test('logged-out reads and writes the global key', async () => {
    stubIndexedDB()

    await storeTheme('light')

    expect(await getTheme()).toBe('light')
  })

  test('a per-user save is scoped and leaves the global key untouched', async () => {
    stubIndexedDB()
    await storeTheme('dark')

    await storeTheme('light', 'alice')

    expect(await getTheme('alice')).toBe('light')
    expect(await getTheme()).toBe('dark')
  })

  test('a per-user read falls back to the global key (legacy migration)', async () => {
    stubIndexedDB()
    // Pre-scoping value: only the global key exists.
    await storeTheme('light')

    expect(await getTheme('alice')).toBe('light')
  })

  test('a stored per-user theme wins over the global key', async () => {
    stubIndexedDB()
    await storeTheme('dark')
    await storeTheme('light', 'alice')

    expect(await getTheme('alice')).toBe('light')
    expect(await getTheme()).toBe('dark')
  })

  test('one user never reads another user scoped key', async () => {
    stubIndexedDB()
    await storeTheme('light', 'alice')
    // No global key and no key for bob: nothing stored for him.
    expect(await getTheme('bob')).toBeNull()
  })

  test('deleteTheme removes only the scoped key', async () => {
    stubIndexedDB()
    await storeTheme('dark')
    await storeTheme('light', 'alice')

    await deleteTheme('alice')

    // The scoped key is gone, so the fallback surfaces the global value.
    expect(await getTheme('alice')).toBe('dark')
    expect(await getTheme('bob')).toBe('dark')
  })

  test('deleteTheme() removes the global key', async () => {
    stubIndexedDB()
    await storeTheme('light')

    await deleteTheme()

    expect(await getTheme()).toBeNull()
  })

  test('a non-theme value under the key resolves to null', async () => {
    const data = stubIndexedDB()
    data.set('theme', 'blue')

    expect(await getTheme()).toBeNull()
  })

  test('reads and writes resolve when IndexedDB is unavailable', async () => {
    // No IndexedDB at all (node env, blocked storage): the store resolves
    // null/void instead of rejecting, so callers fall back to defaults.
    vi.stubGlobal('indexedDB', undefined)

    expect(await getTheme('alice')).toBeNull()
    await expect(storeTheme('light', 'alice')).resolves.toBeUndefined()
    await expect(deleteTheme('alice')).resolves.toBeUndefined()
  })
})
