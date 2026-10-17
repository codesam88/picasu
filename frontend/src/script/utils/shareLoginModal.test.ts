import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/// ShareLoginModal has no mount harness under node vitest (same reason
/// routes.test.ts checks source instead of importing it), so the
/// single-owner display contract is pinned structurally.
const source = readFileSync(
  resolve(import.meta.dirname, '../../components/Modal/ShareLoginModal.vue'),
  'utf-8'
)

describe('ShareLoginModal single-owner display', () => {
  test('share 403 pushes no local snackbar (interceptor owns share-403 display)', () => {
    expect(source).not.toMatch(/messageStore\.(error|push|info|success)/)
  })

  test('share 403 still marks the link expired locally', () => {
    expect(source).toMatch(/isLinkExpired = true/)
  })

  test('wrong-password stays an inline field error', () => {
    expect(source).toMatch(/Incorrect password/)
  })

  test('unexpected failures stay inline without a snackbar', () => {
    expect(source).toMatch(/An unexpected error occurred/)
  })
})
