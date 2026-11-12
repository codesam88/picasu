import { describe, expect, test } from 'vitest'
import { formatBytes } from './formatBytes'

describe('formatBytes', () => {
  test('bytes below 1 KiB stay raw', () => {
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(1023)).toBe('1023 B')
  })

  test('KiB range keeps one decimal', () => {
    expect(formatBytes(1024)).toBe('1.0 KB')
    expect(formatBytes(1536)).toBe('1.5 KB')
  })

  test('MiB range keeps one decimal', () => {
    expect(formatBytes(1024 * 1024)).toBe('1.0 MB')
    expect(formatBytes(3 * 1024 * 1024)).toBe('3.0 MB')
  })
})
