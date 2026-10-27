import { describe, expect, test } from 'vitest'
import { inviteStatus } from './inviteStatus'
import type { InviteView } from '@/api/invites'

const base = (over: Partial<InviteView> = {}): InviteView => ({
  id: 'id',
  prefix: 'pref',
  createdBy: 'admin',
  expiresAt: 9999999999,
  usedBy: null,
  username: null,
  ...over
})

describe('inviteStatus', () => {
  test('unused when not used and not expired', () => {
    expect(inviteStatus(base(), 1000)).toBe('unused')
  })
  test('used when usedBy is set', () => {
    expect(inviteStatus(base({ usedBy: 'bob' }), 1000)).toBe('used')
  })
  test('expired when expiresAt is in the past', () => {
    expect(inviteStatus(base({ expiresAt: 500 }), 1000)).toBe('expired')
  })
  test('revoked when revoked flag is true', () => {
    expect(inviteStatus(base(), 1000, true)).toBe('revoked')
  })
  test('revoked takes precedence over used', () => {
    expect(inviteStatus(base({ usedBy: 'bob' }), 1000, true)).toBe('revoked')
  })
  test('never-expiring invite stays unused', () => {
    expect(inviteStatus(base({ expiresAt: null }), 1000)).toBe('unused')
  })
})
