import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const source = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/Config/ChangePassword.vue'),
  'utf-8'
)

describe('ChangePassword rework', () => {
  test('has no enable/disable toggle or hasPassword model', () => {
    expect(source).not.toContain('hasPassword')
    expect(source).not.toContain('has-password')
    expect(source).not.toContain('Enable Password Protection')
    expect(source).not.toContain('isDisabling')
  })

  test('has no empty-submit disable path', () => {
    expect(source).not.toMatch(/\?\s*(''|"")/)
    expect(source).not.toContain('Password disabled')
    expect(source).not.toMatch(/updatePassword\(oldPassword\.value,\s*finalNewPassword\)/)
  })

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
})
