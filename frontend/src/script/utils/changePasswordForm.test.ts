import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const source = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/Config/ChangePassword.vue'),
  'utf-8'
)

describe('ChangePassword rework', () => {
  test('form has current, new and confirm password fields', () => {
    expect(source).toMatch(/current password/i)
    expect(source).toMatch(/new password/i)
    expect(source).toMatch(/confirm/i)
  })

  test('submits the guarded change with the current and new passwords', () => {
    expect(source).toMatch(
      /submitPasswordChange\(\s*oldPassword\.value,\s*newPassword\.value,\s*confirmPassword\.value/
    )
  })

  test('calls the canonical users/password route with the session userId', () => {
    expect(source).toMatch(/from '@\/api\/users'/)
    expect(source).toMatch(/setUserPassword/)
    expect(source).toMatch(/useCurrentUser|useSessionStore/)
  })

  test('no longer calls the deprecated config/password alias', () => {
    expect(source).not.toMatch(/updatePassword/)
    expect(source).not.toMatch(/put\/config\/password/)
  })
})
