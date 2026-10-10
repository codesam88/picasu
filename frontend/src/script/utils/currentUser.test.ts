import { describe, expect, test } from 'vitest'
import { canSeeUserPanel, errorToMessage, parseCurrentUser, selfRowState } from './currentUser'

// jwt-decode only base64-decodes the payload: unsigned tokens are fine.
const b64url = (s: string): string =>
  Buffer.from(s).toString('base64').replace(/=/g, '').replace(/\+/g, '-').replace(/\//g, '_')

const mint = (payload: unknown): string =>
  `${b64url(JSON.stringify({ alg: 'none' }))}.${b64url(JSON.stringify(payload))}.sig`

describe('parseCurrentUser', () => {
  test('decodes an admin identity token', () => {
    const token = mint({ role: { user: { id: 'alice', admin: true } }, exp: 9999999999 })
    expect(parseCurrentUser(token)).toEqual({ id: 'alice', isAdmin: true })
  })

  test('decodes a non-admin identity token', () => {
    const token = mint({ role: { user: { id: 'bob', admin: false } }, exp: 9999999999 })
    expect(parseCurrentUser(token)).toEqual({ id: 'bob', isAdmin: false })
  })

  test('returns null for a share-role token', () => {
    const token = mint({ role: { share: { albumId: 'a', share: {} } }, exp: 9999999999 })
    expect(parseCurrentUser(token)).toBeNull()
  })

  test('returns null for a legacy admin-role token', () => {
    const token = mint({ role: 'admin', exp: 9999999999 })
    expect(parseCurrentUser(token)).toBeNull()
  })

  test('returns null for garbage, empty, and missing tokens', () => {
    expect(parseCurrentUser('not-a-jwt')).toBeNull()
    expect(parseCurrentUser('')).toBeNull()
    expect(parseCurrentUser(undefined)).toBeNull()
  })

  test('returns null when the user claim is malformed', () => {
    expect(parseCurrentUser(mint({}))).toBeNull()
    expect(parseCurrentUser(mint({ role: {} }))).toBeNull()
    expect(parseCurrentUser(mint({ role: { user: { id: 42, admin: true } } }))).toBeNull()
    expect(parseCurrentUser(mint({ role: { user: { id: 'x', admin: 'yes' } } }))).toBeNull()
  })
})

describe('canSeeUserPanel', () => {
  test('only admins see the panel', () => {
    expect(canSeeUserPanel({ id: 'a', isAdmin: true })).toBe(true)
    expect(canSeeUserPanel({ id: 'b', isAdmin: false })).toBe(false)
    expect(canSeeUserPanel(null)).toBe(false)
  })
})

describe('selfRowState', () => {
  test('self row is locked only when sole admin', () => {
    expect(selfRowState({ id: 'a', isAdmin: true }, 'a', 1)).toBe('sole-admin')
    expect(selfRowState({ id: 'a', isAdmin: true }, 'a', 2)).toBe('ok')
  })

  test('other rows are always actionable', () => {
    expect(selfRowState({ id: 'a', isAdmin: true }, 'b', 1)).toBe('ok')
    expect(selfRowState(null, 'b', 1)).toBe('ok')
  })
})

describe('errorToMessage', () => {
  test('401 signals a login redirect', () => {
    expect(errorToMessage(401)).toEqual({ redirectLogin: true })
  })

  test('409/404/400 surface the server message verbatim', () => {
    expect(errorToMessage(409, 'Cannot demote the last admin')).toEqual({
      redirectLogin: false,
      message: 'Cannot demote the last admin'
    })
    expect(errorToMessage(404, 'User not found')).toEqual({
      redirectLogin: false,
      message: 'User not found'
    })
    expect(errorToMessage(400, 'Invalid user id')).toEqual({
      redirectLogin: false,
      message: 'Invalid user id'
    })
  })

  test('falls back to a generic message when the server sent none', () => {
    const result = errorToMessage(500)
    expect(result.redirectLogin).toBe(false)
    if (!result.redirectLogin) expect(result.message.length).toBeGreaterThan(0)
  })
})
