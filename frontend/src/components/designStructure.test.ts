import { describe, expect, test } from 'vitest'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { unresolvedComponents, usesBaseModal, usesRawDialog } from './designStructure'

const here = fileURLToPath(new URL('.', import.meta.url))
const pageDir = join(here, 'Page')
const modalDir = join(here, 'Modal')

function templateOf(file: string): string {
  const src = readFileSync(file, 'utf8')
  const m = /<template>([\s\S]*)<\/template>/.exec(src)
  return m?.[1] ?? ''
}

function scriptOf(file: string): string {
  const src = readFileSync(file, 'utf8')
  const m = /<script[^>]*>([\s\S]*)<\/script>/.exec(src)
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

  test('unresolvedComponents flags missing imports', () => {
    expect(unresolvedComponents('<BaseModal/>', `import BaseModal from './BaseModal.vue'`)).toEqual(
      []
    )
    expect(unresolvedComponents('<BaseModal/>', '')).toEqual(['BaseModal'])
    expect(unresolvedComponents('<v-dialog/><VDialog/><Transition/>', '')).toEqual([])
  })
})

describe('design structure', () => {
  test('top-level pages render through PageTemplate', () => {
    const offenders = topLevelPages().filter((f) => !templateOf(f).includes('PageTemplate'))
    expect(offenders, `pages bypassing PageTemplate: ${offenders.join(', ')}`).toEqual([])
  })

  test('no modals bypass BaseModal', () => {
    const offenders = modalCandidates().filter((f) => {
      const t = templateOf(f)
      return !usesBaseModal(t)
    })
    expect(offenders, `modals bypassing BaseModal: ${offenders.join(', ')}`).toEqual([])
  })
})

function allComponentFiles(): string[] {
  const out: string[] = []
  const walk = (dir: string): void => {
    for (const f of readdirSync(dir)) {
      const p = join(dir, f)
      if (statSync(p).isDirectory()) walk(p)
      else if (f.endsWith('.vue')) out.push(p)
    }
  }
  walk(here)
  return out
}

describe('component resolution', () => {
  test('all component tags resolve to imports', () => {
    const offenders: string[] = []
    for (const f of allComponentFiles()) {
      const bad = unresolvedComponents(templateOf(f), scriptOf(f))
      if (bad.length > 0) offenders.push(`${f}: ${bad.join(', ')}`)
    }
    expect(offenders, `unresolved component tags: ${offenders.join('; ')}`).toEqual([])
  })
})
