/** Structural design rules for the Vue frontend.
 *
 * Pages render through PageTemplate; dialogs render through BaseModal (or
 * ShareModalBase for share flows). The modal scan below fails on any raw
 * `<v-dialog>` bypass; the historic grandfather list was deleted once it
 * reached zero (all modals migrated).
 */

export function usesBaseModal(template: string): boolean {
  return template.includes('BaseModal') || template.includes('ShareModalBase')
}

export function usesRawDialog(template: string): boolean {
  return template.includes('<v-dialog')
}
