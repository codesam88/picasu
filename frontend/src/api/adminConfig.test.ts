import { describe, expect, test, vi, beforeEach } from 'vitest'
import axios from 'axios'
import {
  getAdminConfig,
  updateAdminConfig,
  disableReadOnly,
  exportAdminConfig
} from './adminConfig'

vi.mock('axios')

const mockedAxios = axios as unknown as {
  get: ReturnType<typeof vi.fn>
  put: ReturnType<typeof vi.fn>
  post: ReturnType<typeof vi.fn>
}

beforeEach(() => {
  vi.resetAllMocks()
})

describe('getAdminConfig', () => {
  test('GETs /get/admin/config and parses entries', async () => {
    mockedAxios.get = vi.fn().mockResolvedValue({
      data: [
        {
          name: 'allow_registration',
          value: true,
          tier: 'runtime',
          source: 'file',
          requiresRestart: false,
          writable: true
        }
      ]
    })
    const result = await getAdminConfig()
    expect(result).toEqual([
      {
        name: 'allow_registration',
        value: true,
        tier: 'runtime',
        source: 'file',
        requiresRestart: false,
        writable: true
      }
    ])
    expect(mockedAxios.get).toHaveBeenCalledWith('/get/admin/config')
  })
})

describe('updateAdminConfig', () => {
  test('PUTs runtime subset to /put/admin/config and parses response', async () => {
    mockedAxios.put = vi.fn().mockResolvedValue({
      data: {
        config: {
          uploadFolder: 'uploads',
          maxUploadSize: '100MiB',
          readOnlyMode: false,
          disableImg: false,
          fsNotifyWatcher: true,
          normalizeUploadFilenames: true,
          validateUploadContent: true,
          useClientTimestampInfo: false,
          allowRegistration: true
        },
        restartRequired: []
      }
    })
    const result = await updateAdminConfig({ allowRegistration: true })
    expect(result.config.allowRegistration).toBe(true)
    expect(result.restartRequired).toEqual([])
    expect(mockedAxios.put).toHaveBeenCalledWith('/put/admin/config', { allowRegistration: true })
  })
})

describe('disableReadOnly', () => {
  test('POSTs no body to disable route', async () => {
    mockedAxios.post = vi.fn().mockResolvedValue({ data: '' })
    await expect(disableReadOnly()).resolves.toBeUndefined()
    expect(mockedAxios.post).toHaveBeenCalledWith('/post/admin/config/disable-read-only')
  })
})

describe('exportAdminConfig', () => {
  test('GETs /get/admin/config/export and parses envelope', async () => {
    mockedAxios.get = vi.fn().mockResolvedValue({
      data: {
        version: 1,
        scope: 'server-runtime',
        config: {
          uploadFolder: 'uploads',
          maxUploadSize: '100MiB',
          readOnlyMode: false,
          disableImg: false,
          fsNotifyWatcher: true,
          normalizeUploadFilenames: true,
          validateUploadContent: true,
          useClientTimestampInfo: false,
          allowRegistration: false
        }
      }
    })
    const result = await exportAdminConfig()
    expect(result.version).toBe(1)
    expect(result.scope).toBe('server-runtime')
    expect(mockedAxios.get).toHaveBeenCalledWith('/get/admin/config/export')
  })
})
