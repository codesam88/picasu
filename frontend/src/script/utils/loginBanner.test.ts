import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const source = readFileSync(
  resolve(import.meta.dirname, '../../components/Login/LoginPage.vue'),
  'utf-8'
)

describe('LoginPage innocuous login banner', () => {
  test('carries a static v-alert banner with the innocuous login copy', () => {
    expect(source).toMatch(/<v-alert/)
    expect(source).toMatch(/Please log in to continue/)
  })

  test('banner is always visible and non-closable', () => {
    // The innocuous banner is static: no v-if gating, no closable prop.
    const bannerSection = source.slice(
      source.indexOf('Please log in to continue') - 400,
      source.indexOf('Please log in to continue')
    )
    expect(bannerSection).toMatch(/<v-alert/)
    expect(bannerSection).not.toMatch(/v-if/)
    expect(bannerSection).not.toMatch(/closable/)
  })
})
