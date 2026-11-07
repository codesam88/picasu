import { describe, expect, test } from 'vitest'
import { readdirSync, readFileSync } from 'node:fs'
import { basename, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { KNOWN_MODAL_DEBT, usesBaseModal, usesRawDialog } from './designStructure'

const here = fileURLToPath(new URL('.', import.meta.url))
const pageDir = join(here, 'Page')
const modalDir = join(here, 'Modal')

function templateOf(file: string): string {
  const src = readFileSync(file, 'utf8')
  const m = /<template>([\s\S]*)<\/template>/.exec(src)
  return m?.[1] ?? ''
}

function topLevelPages(): string[] {
  return readdirSync(pageDir)
    .filter((f) => f.endsWith('.vue'))
    .map((f) => join(pageDir, f))
}

function modalCandidates(): string[] {
  return readdirSync(modalDir)
    .filter((f) => f.endsWith('.vue') && f !== 'BaseModal.vue' && f !== 'ShareModalBase.vue')
    .map((f) => join(modalDir, f))
    .filter((f) => usesRawDialog(templateOf(f)))
}

describe('designStructure helpers', () => {
  test('detects BaseModal usage', () => {
    expect(usesBaseModal('<BaseModal title="x"><p>hi</p></BaseModal>')).toBe(true)
    expect(usesBaseModal('<ShareModalBase><p>hi</p></ShareModalBase>')).toBe(true)
    expect(usesBaseModal('<v-dialog><p>hi</p></v-dialog>')).toBe(false)
  })

  test('detects raw v-dialog bypass', () => {
    expect(usesRawDialog('<v-dialog max-width="400"><p>hi</p></v-dialog>')).toBe(true)
    expect(usesRawDialog('<BaseModal title="x"><p>hi</p></BaseModal>')).toBe(false)
  })
})

describe('design structure', () => {
  test('top-level pages render through PageTemplate', () => {
    const offenders = topLevelPages().filter((f) => !templateOf(f).includes('PageTemplate'))
    expect(offenders, `pages bypassing PageTemplate: ${offenders.join(', ')}`).toEqual([])
  })

  test('no new modals bypass BaseModal beyond known debt', () => {
    const offenders = modalCandidates().filter((f) => {
      const t = templateOf(f)
      return !usesBaseModal(t)
    })
    const fresh = offenders.filter((f) => !KNOWN_MODAL_DEBT.includes(basename(f)))
    expect(fresh, `new modals bypassing BaseModal: ${fresh.join(', ')}`).toEqual([])
  })
})
