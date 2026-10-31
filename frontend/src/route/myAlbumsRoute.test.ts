import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, test } from 'vitest'

const routeDir = fileURLToPath(new URL('.', import.meta.url))

function readRoute(file: string): string {
  return readFileSync(join(routeDir, file), 'utf8')
}

describe('my-albums route', () => {
  test('MyAlbumsPage exists', () => {
    expect(existsSync(join(routeDir, '../components/Page/MyAlbumsPage.vue'))).toBe(true)
  })

  test('routes.ts wires the my-albums route', () => {
    const source = readRoute('routes.ts')
    expect(source).toContain('MyAlbumsPage')
    expect(source).toContain("createRoute('my-albums', MyAlbumsPage)")
    expect(source).toMatch(/'my-albums':\s*'My Albums'/)
  })

  test('createRoute allows my-albums baseName', () => {
    const source = readRoute('createRoute.ts')
    expect(source).toContain("'my-albums'")
  })
})
