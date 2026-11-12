import { describe, expect, test, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import axios, { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import { assignAlbum, assignAlbumBatch, summarizeAssignOutcomes } from './assignAlbum'
import { useMessageStore } from '@/store/messageStore'

beforeEach(() => {
  setActivePinia(createPinia())
  vi.restoreAllMocks()
})

const okResponse = (outcome: string) => ({ status: 200, data: { outcome } })

const axiosFailureWithResponse = (status: number) => {
  const config = { headers: {} } as InternalAxiosRequestConfig
  const response = {
    status,
    statusText: String(status),
    data: { message: 'boom' },
    headers: {},
    config
  }
  return new AxiosError(
    `Request failed with status code ${status}`,
    'ERR_BAD_REQUEST',
    config,
    null,
    response as AxiosResponse
  )
}

describe('summarizeAssignOutcomes', () => {
  test('all moved, plural and singular', () => {
    expect(summarizeAssignOutcomes({ moved: 3, renamed: 0, skipped: 0, failed: 0 })).toBe(
      'Moved 3 files to album.'
    )
    expect(summarizeAssignOutcomes({ moved: 1, renamed: 0, skipped: 0, failed: 0 })).toBe(
      'Moved 1 file to album.'
    )
  })

  test('mixed outcomes list details', () => {
    expect(summarizeAssignOutcomes({ moved: 2, renamed: 1, skipped: 1, failed: 0 })).toBe(
      'Moved 3 files to album (1 file renamed, 1 file skipped).'
    )
  })

  test('nothing moved explains why', () => {
    expect(summarizeAssignOutcomes({ moved: 0, renamed: 0, skipped: 2, failed: 0 })).toBe(
      'No files moved (2 files skipped).'
    )
  })

  test('failures are counted', () => {
    expect(summarizeAssignOutcomes({ moved: 1, renamed: 0, skipped: 0, failed: 2 })).toBe(
      'Moved 1 file to album (2 files failed).'
    )
  })
})

describe('assignAlbum', () => {
  test('single call toasts once and resolves the outcome', async () => {
    vi.spyOn(axios, 'put').mockResolvedValueOnce(okResponse('moved'))
    const outcome = await assignAlbum('asset-1', 'album-1', 0, 'mainId', 'rename')

    expect(outcome).toBe('moved')
    const queue = useMessageStore('mainId').queue
    expect(queue).toHaveLength(1)
    expect(queue[0]?.text).toBe('Moved to album.')
  })

  test('notify:false suppresses the per-file toast', async () => {
    vi.spyOn(axios, 'put').mockResolvedValueOnce(okResponse('renamedFrom'))
    const outcome = await assignAlbum('asset-1', 'album-1', 0, 'mainId', 'rename', {
      notify: false
    })

    expect(outcome).toBe('renamedFrom')
    expect(useMessageStore('mainId').queue).toHaveLength(0)
  })
})

describe('assignAlbumBatch', () => {
  test('one request per file, one summary toast', async () => {
    vi.spyOn(axios, 'put')
      .mockResolvedValueOnce(okResponse('moved'))
      .mockResolvedValueOnce(okResponse('renamedFrom'))
      .mockResolvedValueOnce(okResponse('skipped'))

    await assignAlbumBatch(
      [
        { assetId: 'a1', index: 0 },
        { assetId: 'a2', index: 1 },
        { assetId: 'a3', index: 2 }
      ],
      'album-1',
      'mainId',
      'rename'
    )

    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(axios.put).toHaveBeenCalledTimes(3)
    const queue = useMessageStore('mainId').queue
    expect(queue).toHaveLength(1)
    expect(queue[0]?.text).toBe('Moved 2 files to album (1 file renamed, 1 file skipped).')
  })

  test('a failed file does not stop the rest and is counted', async () => {
    vi.spyOn(axios, 'put')
      .mockResolvedValueOnce(okResponse('moved'))
      .mockRejectedValueOnce(axiosFailureWithResponse(500))
      .mockResolvedValueOnce(okResponse('moved'))

    await assignAlbumBatch(
      [
        { assetId: 'a1', index: 0 },
        { assetId: 'a2', index: 1 },
        { assetId: 'a3', index: 2 }
      ],
      'album-1',
      'mainId',
      'rename'
    )

    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(axios.put).toHaveBeenCalledTimes(3)
    const queue = useMessageStore('mainId').queue
    expect(queue).toHaveLength(1)
    expect(queue[0]?.text).toBe('Moved 2 files to album (1 file failed).')
  })

  test('empty batch toasts nothing and posts nothing', async () => {
    const post = vi.spyOn(axios, 'put')

    await assignAlbumBatch([], 'album-1', 'mainId', 'rename')

    expect(post).not.toHaveBeenCalled()
    expect(useMessageStore('mainId').queue).toHaveLength(0)
  })
})
