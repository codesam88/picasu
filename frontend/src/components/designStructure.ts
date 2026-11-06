/** Structural design rules for the Vue frontend.
 *
 * Pages render through PageTemplate; dialogs render through BaseModal (or
 * ShareModalBase for share flows). Files listed in KNOWN_MODAL_DEBT predate
 * the rule and are grandfathered so the test only fails on new bypasses.
 */

export function usesBaseModal(template: string): boolean {
  return template.includes('BaseModal') || template.includes('ShareModalBase')
}

export function usesRawDialog(template: string): boolean {
  return template.includes('<v-dialog')
}

/** Basenames (not paths) grandfathered until migrated to BaseModal. Empty: all debt migrated. */
export const KNOWN_MODAL_DEBT: string[] = []
