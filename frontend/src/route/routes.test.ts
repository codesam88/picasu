import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, test } from 'vitest'

const routeDir = fileURLToPath(new URL('.', import.meta.url))

function read(file: string): string {
  return readFileSync(join(routeDir, file), 'utf8')
}

/**
 * The favorite/archived pages were removed together with the flags they
 * exposed. These guards keep them from being re-registered: the router module
 * is browser-bound (createWebHistory), so the source is checked instead of
 * importing it.
 */
describe('removed favorite and archived pages', () => {
  test('page components are deleted', () => {
    expect(existsSync(join(routeDir, '../components/Page/FavoritePage.vue'))).toBe(false)
    expect(existsSync(join(routeDir, '../components/Page/ArchivedPage.vue'))).toBe(false)
  })

  test('routes.ts registers no favorite or archived route', () => {
    expect(read('routes.ts')).not.toMatch(/favorite/i)
    expect(read('routes.ts')).not.toMatch(/archived/i)
  })

  test('baseName unions no longer list favorite or archived', () => {
    for (const file of ['createRoute.ts', 'pageReturnType.ts']) {
      expect(read(file)).not.toMatch(/'(favorite|archived)'/)
    }
  })
})
