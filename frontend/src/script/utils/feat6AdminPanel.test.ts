/// Structural pins for the admin-panel glue, same convention as
/// userManagement.test.ts: the .vue files have no mount harness under
/// node vitest (deliberate — vitest.config.ts pins environment: node),
/// so template-to-handler wiring is pinned on the source while behavior
/// contracts live at the api layer (users.test.ts, invites.test.ts) and
/// in extracted utils (inviteStatus.test.ts).
import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const userMgmt = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/Config/UserManagement.vue'),
  'utf-8'
)
const inviteMgmt = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/Config/InviteManagement.vue'),
  'utf-8'
)
const configPage = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/ConfigPage.vue'),
  'utf-8'
)

describe('FEAT6 user management panel', () => {
  test('create form calls createUser and refreshes the list', () => {
    expect(userMgmt).toMatch(/createUser\(/)
    expect(userMgmt).toMatch(/refreshUsers\(\)/)
  })

  test('non-admin hides create/password/delete controls', () => {
    expect(userMgmt).toMatch(/v-if="isAdmin"/)
  })

  test('elevation calls setUserAdmin', () => {
    expect(userMgmt).toMatch(/setUserAdmin\(/)
  })

  test('set-password calls setUserPassword with the new signature', () => {
    expect(userMgmt).toMatch(/setUserPassword\(/)
  })

  test('set-password success clears the dialog state', () => {
    expect(userMgmt).toMatch(/setPasswordOpen\.value = false/)
    expect(userMgmt).toMatch(/newSetPassword\.value = ''/)
    expect(userMgmt).toMatch(/adminPassword\.value = ''/)
  })

  test('gap-fill guard surfaces non-axios errors and never redirects', () => {
    expect(userMgmt).not.toMatch(/router\.push/)
    expect(userMgmt).toMatch(/axios\.isAxiosError\(error\)/)
  })
})

describe('FEAT6 invite management panel', () => {
  test('create renders the link via buildInviteLink', () => {
    expect(inviteMgmt).toMatch(/buildInviteLink\(/)
    expect(inviteMgmt).toMatch(/createdLink/)
  })

  test('link is not persisted to storage (shown once)', () => {
    expect(inviteMgmt).not.toMatch(/localStorage/)
    expect(inviteMgmt).not.toMatch(/sessionStorage/)
  })

  test('revoke calls revokeInvite and refreshes', () => {
    expect(inviteMgmt).toMatch(/revokeInvite\(/)
    expect(inviteMgmt).toMatch(/refreshInvites\(\)/)
  })

  test('expiry input parses <integer><unit> against the server bound', () => {
    // The raw-hours number field accepted anything finite; the parser
    // strips spaces, enforces h/d/m and validates against the bound the
    // console fetches when building the panel.
    expect(inviteMgmt).toMatch(/parseExpiryInput\(/)
    expect(inviteMgmt).toMatch(/getInviteLimits\(/)
    expect(inviteMgmt).toMatch(/maxExpiryHours/)
    expect(inviteMgmt).not.toMatch(/type="number"/)
  })

  test('invite rows sit in an inset sub-table, aligned with the form', () => {
    // The create form lives in v-card-text; a bare v-list as a direct card
    // child bleeds edge to edge and reads as outside the frame.
    expect(inviteMgmt).toMatch(/<v-sheet[\s\S]*?border/)
  })

  test('creating an invite never touches the clipboard', () => {
    // The token link is shown once in the alert; writing it to the
    // clipboard unprompted would leak it to whatever reads the clipboard.
    const createStart = inviteMgmt.indexOf('const create = async')
    const createEnd = inviteMgmt.indexOf('const revoke = async')
    expect(createStart).toBeGreaterThanOrEqual(0)
    expect(createEnd).toBeGreaterThan(createStart)
    const createBody = inviteMgmt.slice(createStart, createEnd)
    expect(createBody).not.toMatch(/clipboard/i)
    expect(createBody).not.toMatch(/copyLink|copyTextToClipboard/)
  })

  test('copy uses the secure Clipboard API, gated on a secure context', () => {
    // navigator.clipboard is undefined in insecure contexts (plain HTTP)
    // and execCommand('copy') is deprecated, so the Copy button renders
    // only where the secure API can run; otherwise a manual-copy hint
    // shows instead of a button that always errors.
    expect(inviteMgmt).not.toMatch(/navigator\.clipboard/)
    expect(inviteMgmt).not.toMatch(/execCommand/)
    expect(inviteMgmt).toMatch(/v-if="showCopy"/)
    expect(inviteMgmt).toMatch(/Select and copy manually/)
  })

  test('status derivation is used for display', () => {
    expect(inviteMgmt).toMatch(/inviteStatus\(/)
  })
})

describe('FEAT6 registration toggle', () => {
  test('toggling calls updateAdminConfig with allowRegistration', () => {
    expect(configPage).toMatch(/updateAdminConfig\(\{\s*allowRegistration/)
  })

  test('initial display reads getAdminConfig read-back', () => {
    expect(configPage).toMatch(/getAdminConfig\(\)/)
    expect(configPage).toMatch(/allow_registration/)
  })
})
