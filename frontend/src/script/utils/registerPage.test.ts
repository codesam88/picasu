import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/// RegisterPage is a .vue file with no mount harness under node vitest
/// (same reason routes.test.ts checks source instead of importing it), so
/// its single-owner wiring is pinned structurally. The callback-level
/// behavior (which rejections reach onError) is pinned in
/// registration.test.ts instead.
const source = readFileSync(
  resolve(import.meta.dirname, '../../components/Login/RegisterPage.vue'),
  'utf-8'
)

describe('RegisterPage single-owner display', () => {
  test('race-lost fallback redirects without pushing a second snackbar', () => {
    expect(source).not.toMatch(/messageStore\.info/)
    expect(source).toMatch(/router\.replace\(\{\s*name:\s*'login'\s*\}\)/)
  })

  test('unexpected registration failures still surface locally', () => {
    expect(source).toMatch(/onError/)
    expect(source).toMatch(/messageStore\.error\(errorDisplay\(err\)\)/)
  })
})
