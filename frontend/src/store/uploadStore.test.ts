import { describe, expect, test, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import axios, { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import { buildUploadUrl, useUploadStore } from './uploadStore'
import { useMessageStore } from './messageStore'

beforeEach(() => {
  setActivePinia(createPinia())
  vi.restoreAllMocks()
})

/// Real axios rejection carrying a response: the global response
/// interceptor has already displayed it, so the store must stay silent.
const axiosFailureWithResponse = (status: number) => {
  const config = { headers: {} } as InternalAxiosRequestConfig
  const response = {
    status,
    statusText: String(status),
    data: { message: 'boom' },
    headers: {},
    config
  } as AxiosResponse
  return new AxiosError(
    `Request failed with status code ${status}`,
    'ERR_BAD_REQUEST',
    config,
    null,
    response
  )
}

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

describe('fileUpload failure display ownership', () => {
  test('axios failure with a response pushes no local snackbar', async () => {
    vi.spyOn(axios, 'post').mockRejectedValueOnce(axiosFailureWithResponse(500))
    const store = useUploadStore('mainId')

    await store.fileUpload([], undefined, true)

    expect(store.status).toBe('Canceled')
    expect(useMessageStore('mainId').queue).toHaveLength(0)
  })

  test('responseless axios failure still surfaces locally', async () => {
    const failure = new AxiosError(
      'Network Error',
      'ERR_NETWORK',
      { headers: {} } as InternalAxiosRequestConfig,
      null
    )
    vi.spyOn(axios, 'post').mockRejectedValueOnce(failure)
    const store = useUploadStore('mainId')

    await store.fileUpload([], undefined, true)

    expect(store.status).toBe('Canceled')
    expect(useMessageStore('mainId').queue).toHaveLength(1)
  })

  test('non-axios throw still surfaces locally', async () => {
    vi.spyOn(axios, 'post').mockRejectedValueOnce(new Error('boom'))
    const store = useUploadStore('mainId')

    await store.fileUpload([], undefined, true)

    expect(store.status).toBe('Canceled')
    expect(useMessageStore('mainId').queue).toHaveLength(1)
  })
})
