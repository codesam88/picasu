import { describe, expect, test, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import axios, { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import { buildUploadUrl, useUploadStore } from './uploadStore'
import { useMessageStore } from './messageStore'
import { refreshGalleryAfterMutation } from '@/script/hook/usePrefetch'

vi.mock('@/script/hook/usePrefetch', () => ({
  refreshGalleryAfterMutation: vi.fn()
}))

vi.mock('@/route/routes', () => ({
  default: { currentRoute: { value: {} } }
}))

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
    expect(buildUploadUrl(undefined, true)).toBe('/post/upload?auto_rename=true')
  })

  test('no album, auto_rename false', () => {
    expect(buildUploadUrl(undefined, false)).toBe('/post/upload?auto_rename=false')
  })

  test('with album, auto_rename true', () => {
    expect(buildUploadUrl('album-1', true)).toBe(
      '/post/upload?presigned_album_id_opt=album-1&auto_rename=true'
    )
  })

  test('with album, auto_rename false', () => {
    expect(buildUploadUrl('album-1', false)).toBe(
      '/post/upload?presigned_album_id_opt=album-1&auto_rename=false'
    )
  })

  test('album id is URL-encoded', () => {
    expect(buildUploadUrl('a b/c', true)).toBe(
      '/post/upload?presigned_album_id_opt=a%20b%2Fc&auto_rename=true'
    )
  })
})

describe('fileUpload sequential queue', () => {
  test('posts once per file with a single file part, refreshes once, completes', async () => {
    const file1 = new File(['a'], 'a.png', { type: 'image/png', lastModified: 10 })
    const file2 = new File(['b'], 'b.png', { type: 'image/png', lastModified: 20 })

    vi.spyOn(axios, 'post')
      .mockResolvedValueOnce({ data: { files: ['a.png'], dir: '/img/album-1' }, status: 200 })
      .mockResolvedValueOnce({ data: { files: ['b.png'], dir: '/img/album-1' }, status: 200 })

    const refresh = vi.mocked(refreshGalleryAfterMutation)

    const store = useUploadStore('mainId')
    await store.fileUpload([file1, file2], 'album-1', true)

    // eslint-disable-next-line @typescript-eslint/unbound-method
    const postMock = vi.mocked(axios.post)
    expect(postMock).toHaveBeenCalledTimes(2)
    const calls = postMock.mock.calls
    const firstForm = calls[0]?.[1] as FormData
    const secondForm = calls[1]?.[1] as FormData
    expect(firstForm.getAll('file')).toHaveLength(1)
    expect(secondForm.getAll('file')).toHaveLength(1)

    // Store-and-return: the 200 is the completion signal — no polling.
    expect(store.status).toBe('Completed')
    expect(refresh).toHaveBeenCalledTimes(1)
  })

  test('mid-queue failure stops further posts and cancels', async () => {
    const file1 = new File(['a'], 'a.png', { type: 'image/png', lastModified: 10 })
    const file2 = new File(['b'], 'b.png', { type: 'image/png', lastModified: 20 })
    const file3 = new File(['c'], 'c.png', { type: 'image/png', lastModified: 30 })

    vi.spyOn(axios, 'post')
      .mockResolvedValueOnce({ data: { files: ['a.png'] }, status: 200 })
      .mockRejectedValueOnce(axiosFailureWithResponse(500))

    const store = useUploadStore('mainId')
    await store.fileUpload([file1, file2, file3], undefined, true)

    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(axios.post).toHaveBeenCalledTimes(2)
    expect(store.status).toBe('Canceled')
    expect(useMessageStore('mainId').queue).toHaveLength(0)
  })

  test('cancel aborts the in-flight request', async () => {
    const file1 = new File(['a'], 'a.png', { type: 'image/png', lastModified: 10 })

    let rejectFn: (err: unknown) => void = () => {
      // noop default
    }
    vi.spyOn(axios, 'post').mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          rejectFn = reject
        })
    )

    const store = useUploadStore('mainId')
    const promise = store.fileUpload([file1], undefined, true)

    store.cancelUpload()
    rejectFn(
      new AxiosError('canceled', 'ERR_CANCELED', { headers: {} } as InternalAxiosRequestConfig)
    )

    await promise
    expect(store.status).toBe('Canceled')
  })
})

describe('fileUpload failure display ownership', () => {
  test('axios failure with a response pushes no local snackbar', async () => {
    const file1 = new File(['a'], 'a.png', { type: 'image/png', lastModified: 10 })
    vi.spyOn(axios, 'post').mockRejectedValueOnce(axiosFailureWithResponse(500))
    const store = useUploadStore('mainId')

    await store.fileUpload([file1], undefined, true)

    expect(store.status).toBe('Canceled')
    expect(useMessageStore('mainId').queue).toHaveLength(0)
  })

  test('responseless axios failure still surfaces locally', async () => {
    const file1 = new File(['a'], 'a.png', { type: 'image/png', lastModified: 10 })
    const failure = new AxiosError(
      'Network Error',
      'ERR_NETWORK',
      { headers: {} } as InternalAxiosRequestConfig,
      null
    )
    vi.spyOn(axios, 'post').mockRejectedValueOnce(failure)
    const store = useUploadStore('mainId')

    await store.fileUpload([file1], undefined, true)

    expect(store.status).toBe('Canceled')
    expect(useMessageStore('mainId').queue).toHaveLength(1)
  })

  test('non-axios throw still surfaces locally', async () => {
    const file1 = new File(['a'], 'a.png', { type: 'image/png', lastModified: 10 })
    vi.spyOn(axios, 'post').mockRejectedValueOnce(new Error('boom'))
    const store = useUploadStore('mainId')

    await store.fileUpload([file1], undefined, true)

    expect(store.status).toBe('Canceled')
    expect(useMessageStore('mainId').queue).toHaveLength(1)
  })
})
