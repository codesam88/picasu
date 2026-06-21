import type { Covers, UiAssertItem } from './types'

export interface APIRecord {
  method: string
  path: string
}

export interface UIRecord {
  verb: string
  target: string
}

export interface CoverageWarning {
  type: 'missing_api' | 'missing_ui'
  expected: string
}

export class CoverageTracer {
  apiCalls: APIRecord[] = []
  uiCalls: UIRecord[] = []

  recordAPI(method: string, path: string): void {
    this.apiCalls.push({ method, path })
  }

  recordUI(verb: string, target: string): void {
    this.uiCalls.push({ verb, target })
  }

  compare(covers: Covers): CoverageWarning[] {
    const warnings: CoverageWarning[] = []

    for (const expected of covers.api ?? []) {
      const trimmed = expected.trim()
      const spaceIdx = trimmed.indexOf(' ')
      if (spaceIdx === -1) {
        warnings.push({ type: 'missing_api', expected: trimmed })
        continue
      }
      const method = trimmed.slice(0, spaceIdx)
      const path = trimmed.slice(spaceIdx + 1)
      const found = this.apiCalls.some((c) => c.method === method && c.path === path)
      if (!found) {
        warnings.push({ type: 'missing_api', expected: trimmed })
      }
    }

    for (const expected of covers.ui ?? []) {
      const found = this.uiCalls.some((c) => c.target === expected)
      if (!found) {
        warnings.push({ type: 'missing_ui', expected })
      }
    }

    return warnings
  }

  reset(): void {
    this.apiCalls = []
    this.uiCalls = []
  }
}

export function assertionTarget(assertion: UiAssertItem): string {
  if ('ui.visible' in assertion) return assertion['ui.visible']
  if ('ui.hidden' in assertion) return assertion['ui.hidden']
  if ('ui.text' in assertion) return assertion['ui.text']
  if ('ui.route' in assertion) return `route:${assertion['ui.route']}`
  if ('ui.modal' in assertion) return `modal:${assertion['ui.modal']}`
  if ('ui.toast' in assertion) {
    const t = assertion['ui.toast']
    return `toast:${t.type}:${t.contains}`
  }
  if ('ui.aria_snapshot' in assertion) return `snapshot:${assertion['ui.aria_snapshot']}`
  if ('ui.text_visible' in assertion) return assertion['ui.text_visible']
  if ('ui.count' in assertion) return assertion['ui.count']
  if ('ui.sidebar_visible' in assertion) return assertion['ui.sidebar_visible']
  if ('ui.chip_visible' in assertion) return assertion['ui.chip_visible']
  return ''
}
