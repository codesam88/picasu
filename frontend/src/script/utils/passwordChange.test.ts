import { describe, expect, test, vi } from 'vitest'
import { isPasswordChangeSubmittable, submitPasswordChange } from './passwordChange'

describe('isPasswordChangeSubmittable', () => {
  test('blocks submit when new and confirm differ', () => {
    expect(isPasswordChangeSubmittable('old-secret', 'new-secret', 'other-secret')).toBe(false)
  })

  test('blocks submit when the new password is empty', () => {
    expect(isPasswordChangeSubmittable('old-secret', '', '')).toBe(false)
  })

  test('blocks submit when the current password is empty', () => {
    expect(isPasswordChangeSubmittable('', 'new-secret', 'new-secret')).toBe(false)
  })

  test('allows submit when current is non-empty and new and confirm match', () => {
    expect(isPasswordChangeSubmittable('old-secret', 'new-secret', 'new-secret')).toBe(true)
  })
})

describe('submitPasswordChange', () => {
  test('mismatch never calls update', async () => {
    const update = vi.fn().mockResolvedValue(undefined)

    await expect(
      submitPasswordChange('old-secret', 'new-secret', 'other-secret', update)
    ).resolves.toBe(false)
    expect(update).not.toHaveBeenCalled()
  })

  test('empty new password never calls update', async () => {
    const update = vi.fn().mockResolvedValue(undefined)

    await expect(submitPasswordChange('old-secret', '', '', update)).resolves.toBe(false)
    expect(update).not.toHaveBeenCalled()
  })

  test('empty current password never calls update', async () => {
    const update = vi.fn().mockResolvedValue(undefined)

    await expect(submitPasswordChange('', 'new-secret', 'new-secret', update)).resolves.toBe(false)
    expect(update).not.toHaveBeenCalled()
  })

  test('matching passwords submit current and new passwords', async () => {
    const update = vi.fn().mockResolvedValue(undefined)

    await expect(
      submitPasswordChange('old-secret', 'new-secret', 'new-secret', update)
    ).resolves.toBe(true)
    expect(update).toHaveBeenCalledTimes(1)
    expect(update).toHaveBeenCalledWith('old-secret', 'new-secret')
  })
})
