/** Structural design rules for the Vue frontend.
 *
 * Pages render through PageTemplate; dialogs render through BaseModal (or
 * ShareModalBase for share flows). The modal scan below fails on any raw
 * `<v-dialog>` bypass. Every non-Vuetify component tag must resolve to a
 * local import: an unimported tag silently renders as a native custom
 * element, inlining its slot content with no overlay, chrome, or actions.
 */

export function usesBaseModal(template: string): boolean {
  return template.includes('BaseModal') || template.includes('ShareModalBase')
}

export function usesRawDialog(template: string): boolean {
  return template.includes('<v-dialog')
}

/** Vue built-ins usable as tags without import. */
const BUILTIN_TAGS = new Set([
  'Transition',
  'TransitionGroup',
  'Teleport',
  'KeepAlive',
  'Suspense',
  'component'
])

/** PascalCase tags in a template that are neither Vue built-ins nor Vuetify
 * (`V` + uppercase, globally registered by the plugin) must be imported by
 * the SFC. Returns the unresolved names. */
export function unresolvedComponents(template: string, script: string): string[] {
  const imported = new Set<string>()
  // Default imports: `import X from '…'`, `import X, {…} from '…'`
  for (const m of script.matchAll(/import\s+(?:type\s+)?(\w+)(?=\s*(?:,|\s+from\b))/g)) {
    const def = m[1]
    if (def !== undefined) imported.add(def)
  }
  // Named imports: `import { A, B as C } from '…'` (aliases bind the last name)
  for (const m of script.matchAll(/import\s+(?:type\s+)?(?:\w+\s*,\s*)?\{([^}]*)\}/g)) {
    const names = m[1]
    if (names === undefined) continue
    for (const named of names.split(',')) {
      const parts = named
        .split(' as ')
        .map((s) => s.trim())
        .filter((s) => s !== '')
      const last = parts[parts.length - 1]
      if (last !== undefined) imported.add(last)
    }
  }
  // Namespace imports: `import * as X from '…'`
  for (const m of script.matchAll(/import\s+(?:type\s+)?\*\s*as\s+(\w+)/g)) {
    const ns = m[1]
    if (ns !== undefined) imported.add(ns)
  }
  const tags = new Set<string>()
  for (const m of template.matchAll(/<([A-Z][A-Za-z0-9]*)/g)) {
    const tag = m[1]
    if (tag !== undefined) tags.add(tag)
  }
  return [...tags].filter((t) => !BUILTIN_TAGS.has(t) && !/^V[A-Z]/.test(t) && !imported.has(t))
}
