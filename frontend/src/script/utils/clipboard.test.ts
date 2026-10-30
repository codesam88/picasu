/// copyTextToClipboard: secure Clipboard API only, gated on a secure
/// context (https). No execCommand fallback — it is deprecated and must
/// not be the default copy path.
import { describe, expect, test } from 'vitest'
import { canShowCopyButton, copyTextToClipboard } from './clipboard'

describe('canShowCopyButton', () => {
  test('true only with a secure context and a Clipboard API', () => {
    expect(
      canShowCopyButton({ secureContext: true, clipboard: { writeText: async () => {} } })
    ).toBe(true)
    expect(canShowCopyButton({ secureContext: false })).toBe(false)
    expect(canShowCopyButton({ secureContext: true })).toBe(false)
    expect(
      canShowCopyButton({ secureContext: false, clipboard: { writeText: async () => {} } })
    ).toBe(false)
  })
})

describe('copyTextToClipboard', () => {
  test('resolves true when the secure write succeeds', async () => {
    let written = ''
    const ok = await copyTextToClipboard('abc', {
      secureContext: true,
      clipboard: {
        writeText: async (text: string) => {
          written = text
        }
      }
    })
    expect(ok).toBe(true)
    expect(written).toBe('abc')
  })

  test('never writes outside a secure context', async () => {
    let written = ''
    const ok = await copyTextToClipboard('abc', {
      secureContext: false,
      clipboard: {
        writeText: async (text: string) => {
          written = text
        }
      }
    })
    expect(ok).toBe(false)
    expect(written).toBe('')
  })

  test('returns false when the API is missing or rejects', async () => {
    await expect(copyTextToClipboard('abc', { secureContext: true })).resolves.toBe(false)
    await expect(
      copyTextToClipboard('abc', {
        secureContext: true,
        clipboard: {
          writeText: async () => {
            throw new Error('denied')
          }
        }
      })
    ).resolves.toBe(false)
  })

  test('default globals resolve false without throwing where no DOM exists', async () => {
    // Under node vitest there is no window/navigator; the util must still
    // settle instead of throwing on property access.
    await expect(copyTextToClipboard('abc')).resolves.toBe(false)
    expect(canShowCopyButton()).toBe(false)
  })
})
