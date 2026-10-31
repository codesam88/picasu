import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/// The main shares table keeps one compact `Access` column instead of four
/// per-property columns. Scoping (whose shares render) is enforced
/// server-side — the table renders whatever the endpoint returns.
const source = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/LinksPage.vue'),
  'utf-8'
)

describe('shares table Access column', () => {
  test('collapses the four property columns into one Access column', () => {
    expect(source).toMatch(/title: 'Access'/)
    expect(source).toMatch(/key: 'access'/)
    expect(source).not.toMatch(/title: 'Password'/)
    expect(source).not.toMatch(/title: 'Download'/)
    expect(source).not.toMatch(/title: 'Upload'/)
    expect(source).not.toMatch(/title: 'Metadata'/)
  })

  test('icons carry state color and tooltips', () => {
    expect(source).toMatch(/item\.access/)
    expect(source).toMatch(/mdi-lock/)
    expect(source).toMatch(/mdi-download/)
    expect(source).toMatch(/mdi-upload/)
    expect(source).toMatch(/mdi-tag/)
    expect(source).toMatch(/'success'/)
  })
})
