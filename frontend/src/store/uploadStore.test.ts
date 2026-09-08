import { describe, expect, test } from 'vitest'
import { buildUploadUrl } from './uploadStore'

describe('buildUploadUrl', () => {
  test('no album, auto_rename true', () => {
    expect(buildUploadUrl(undefined, true)).toBe('/upload?auto_rename=true')
  })

  test('no album, auto_rename false', () => {
    expect(buildUploadUrl(undefined, false)).toBe('/upload?auto_rename=false')
  })

  test('with album, auto_rename true', () => {
    expect(buildUploadUrl('album-1', true)).toBe(
      '/upload?presigned_album_id_opt=album-1&auto_rename=true'
    )
  })

  test('with album, auto_rename false', () => {
    expect(buildUploadUrl('album-1', false)).toBe(
      '/upload?presigned_album_id_opt=album-1&auto_rename=false'
    )
  })

  test('album id is URL-encoded', () => {
    expect(buildUploadUrl('a b/c', true)).toBe(
      '/upload?presigned_album_id_opt=a%20b%2Fc&auto_rename=true'
    )
  })
})
