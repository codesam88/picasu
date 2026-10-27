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
