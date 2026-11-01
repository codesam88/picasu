import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/// CreateShareModal has no mount harness under node vitest (same reason
/// shareLoginModal.test.ts checks source instead of importing it), so the
/// single-owner display contract is pinned structurally.
const source = readFileSync(
  resolve(import.meta.dirname, '../../components/Modal/CreateShareModal.vue'),
  'utf-8'
)

describe('CreateShareModal single-owner display', () => {
  test('create path has no local error snackbar (interceptor owns axios display)', () => {
    expect(source).not.toMatch(/messageStore\.error\(/)
  })

  test('create path routes through tryWithMessageStore', () => {
    expect(source).toMatch(
      /tryWithMessageStore\('mainId', async \(\) => \{[\s\S]*?axios\.post<\{ link: string; fingerprint: string \}>\('\/post\/create_share'/
    )
  })

  test('create path still shows the success snackbar', () => {
    expect(source).toMatch(/messageStore\.success\('Share link created successfully\.'\)/)
  })

  test('create path still resets loading in finally', () => {
    expect(source).toMatch(/const createLink[\s\S]*?\} finally \{\s*loading\.value = false\s*\}/)
  })

  test('updateLink addresses the created share by fingerprint, not bearer', () => {
    expect(source).toMatch(/url: createdFingerprint\.value/)
    expect(source).not.toMatch(/url: createdShareKey\.value/)
  })
})
