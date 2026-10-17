import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/// UserManagement is a .vue file with no mount harness under node vitest
/// (same reason routes.test.ts checks source instead of importing it), so
/// the removal of its duplicate 401 navigation is pinned structurally.
const source = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/Config/UserManagement.vue'),
  'utf-8'
)

describe('UserManagement single-owner display', () => {
  test('no duplicate navigation to login (interceptor owns the redirect)', () => {
    expect(source).not.toMatch(/router\.push/)
    expect(source).not.toMatch(/redirectLogin/)
  })

  test('gap-fill guard still toasts only what the interceptor could not display', () => {
    expect(source).toMatch(/axios\.isAxiosError\(error\)/)
    expect(source).toMatch(/messageStore\.error\(errorDisplay\(error\)\)/)
  })
})
