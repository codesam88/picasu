import { describe, expect, test, beforeEach, afterEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import axios, { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import { useTokenStore, RENEW_BUFFER_SECS } from './tokenStore'

describe('tokenStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  describe('asset-based token identity', () => {
    test('two same-hash assets with different assetIds have separate token entries', () => {
      const tokenStore = useTokenStore('mainId')

      const token1 =
        'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ0aW1lc3RhbXAiOjE3MDAwMDAwMDAsImV4cCI6MTcwMDAwMDkwMH0.abc1'
      const token2 =
        'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ0aW1lc3RhbXAiOjE3MDAwMDAwMDAsImV4cCI6MTcwMDAwMDkwMH0.abc2'

      tokenStore.assetTokenMap.set('asset_a', token1)
      tokenStore.assetTokenMap.set('asset_b', token2)

      expect(tokenStore.assetTokenMap.get('asset_a')).toBe(token1)
      expect(tokenStore.assetTokenMap.get('asset_b')).toBe(token2)
      expect(tokenStore.assetTokenMap.get('asset_a')).not.toBe(
        tokenStore.assetTokenMap.get('asset_b')
      )
    })

    test('cross-asset token lookup by different assetId returns undefined', () => {
      const tokenStore = useTokenStore('mainId')

      tokenStore.assetTokenMap.set('asset_a', 'token_for_a')

      expect(tokenStore.assetTokenMap.get('asset_b')).toBeUndefined()
    })

    test('token refresh updates correct asset entry', () => {
      const tokenStore = useTokenStore('mainId')

      tokenStore.assetTokenMap.set('asset_a', 'old_token_a')
      tokenStore.assetTokenMap.set('asset_b', 'old_token_b')

      tokenStore.assetTokenMap.set('asset_a', 'new_token_a')

      expect(tokenStore.assetTokenMap.get('asset_a')).toBe('new_token_a')
      expect(tokenStore.assetTokenMap.get('asset_b')).toBe('old_token_b')
    })

    test('album cover tokens are keyed by the cover asset id', () => {
      const tokenStore = useTokenStore('mainId')

      tokenStore.assetTokenMap.set('cover_asset', 'cover_token')

      expect(tokenStore.assetTokenMap.get('cover_asset')).toBe('cover_token')
    })
  })

  // jwtDecode only reads the payload segment; the signature is never
  // verified client-side, so fixtures use a dummy '.sig' suffix.
  const b64url = (value: unknown) => Buffer.from(JSON.stringify(value)).toString('base64url')
  const makeJwt = (exp: number, timestamp = 1700000000, caller = '') =>
    `${b64url({ alg: 'HS256', typ: 'JWT' })}.${b64url({ timestamp, exp, caller })}.sig`

  // Fixed wall clock so "seconds of validity left" is deterministic.
  const NOW_MS = 1_750_000_000_000
  const nowSec = Math.floor(NOW_MS / 1000)

  interface SeenRequest {
    url?: string
    data: unknown
    auth: string | undefined
  }

  const authOf = (config: InternalAxiosRequestConfig): string | undefined => {
    const headers = config.headers as unknown as {
      get?: (name: string) => string
      Authorization?: string
    }
    if (typeof headers?.get === 'function') return headers.get('Authorization')
    return headers?.Authorization
  }

  const record = (seen: SeenRequest[], config: InternalAxiosRequestConfig) => {
    seen.push({
      url: config.url,
      data: typeof config.data === 'string' ? JSON.parse(config.data) : config.data,
      auth: authOf(config)
    })
  }

  const okResponse = (config: InternalAxiosRequestConfig, token: string): AxiosResponse => ({
    data: { token },
    status: 200,
    statusText: 'OK',
    headers: {},
    config
  })

  describe('renew buffer gate', () => {
    const originalAdapter = axios.defaults.adapter
    let seen: SeenRequest[]

    beforeEach(() => {
      setActivePinia(createPinia())
      vi.useFakeTimers()
      vi.setSystemTime(NOW_MS)
      seen = []
      axios.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
        record(seen, config)
        return okResponse(config, makeJwt(nowSec + 300))
      }
    })

    afterEach(() => {
      axios.defaults.adapter = originalAdapter
      vi.useRealTimers()
    })

    test('pins RENEW_BUFFER_SECS to 120', () => {
      expect(RENEW_BUFFER_SECS).toBe(120)
    })

    test('renews the timestamp token when 119s of validity remain', async () => {
      const tokenStore = useTokenStore('mainId')
      const stale = makeJwt(nowSec + 119)
      tokenStore.timestampToken = stale

      await tokenStore.refreshTimestampTokenIfExpired()

      expect(seen).toHaveLength(1)
      expect(seen[0]?.url).toBe('/post/renew-timestamp-token')
      expect(seen[0]?.data).toEqual({ token: stale })
      expect(tokenStore.timestampToken).toBe(makeJwt(nowSec + 300))
    })

    test('does not renew the timestamp token when 121s of validity remain', async () => {
      const tokenStore = useTokenStore('mainId')
      const fresh = makeJwt(nowSec + 121)
      tokenStore.timestampToken = fresh

      await tokenStore.refreshTimestampTokenIfExpired()

      expect(seen).toHaveLength(0)
      expect(tokenStore.timestampToken).toBe(fresh)
    })
  })

  describe('forceRenewTimestampToken', () => {
    const originalAdapter = axios.defaults.adapter
    let seen: SeenRequest[]

    beforeEach(() => {
      setActivePinia(createPinia())
      vi.useFakeTimers()
      vi.setSystemTime(NOW_MS)
      seen = []
      axios.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
        record(seen, config)
        return okResponse(config, makeJwt(nowSec + 300))
      }
    })

    afterEach(() => {
      axios.defaults.adapter = originalAdapter
      vi.useRealTimers()
    })

    test('renews unconditionally with 15 min of local validity left', async () => {
      const tokenStore = useTokenStore('mainId')
      tokenStore.timestampToken = makeJwt(nowSec + 900)

      await tokenStore.forceRenewTimestampToken()

      expect(seen).toHaveLength(1)
      expect(seen[0]?.url).toBe('/post/renew-timestamp-token')
      expect(tokenStore.timestampToken).toBe(makeJwt(nowSec + 300))
    })

    test('two concurrent calls single-flight to one POST', async () => {
      const tokenStore = useTokenStore('mainId')
      tokenStore.timestampToken = makeJwt(nowSec + 900)
      let release!: () => void
      const gate = new Promise<void>((resolve) => {
        release = resolve
      })
      axios.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
        record(seen, config)
        await gate
        return okResponse(config, makeJwt(nowSec + 300))
      }

      const first = tokenStore.forceRenewTimestampToken()
      const second = tokenStore.forceRenewTimestampToken()
      release()
      await Promise.all([first, second])

      expect(seen).toHaveLength(1)
      expect(tokenStore.timestampToken).toBe(makeJwt(nowSec + 300))
    })
  })

  describe('forceRenewAssetToken', () => {
    const originalAdapter = axios.defaults.adapter
    let seen: SeenRequest[]

    beforeEach(() => {
      setActivePinia(createPinia())
      vi.useFakeTimers()
      vi.setSystemTime(NOW_MS)
      seen = []
    })

    afterEach(() => {
      axios.defaults.adapter = originalAdapter
      vi.useRealTimers()
    })

    test('renews unconditionally, posting the bearer and body exactly as today', async () => {
      const tokenStore = useTokenStore('mainId')
      const timestampToken = makeJwt(nowSec + 900)
      const currentAsset = makeJwt(nowSec + 900, 1700000001)
      const renewedAsset = makeJwt(nowSec + 300, 1700000001)
      tokenStore.timestampToken = timestampToken
      tokenStore.assetTokenMap.set('asset_a', currentAsset)
      axios.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
        record(seen, config)
        return okResponse(config, renewedAsset)
      }

      const result = await tokenStore.forceRenewAssetToken('asset_a')

      expect(seen).toHaveLength(1)
      expect(seen[0]?.url).toBe('/post/renew-hash-token')
      expect(seen[0]?.auth).toBe(`Bearer ${timestampToken}`)
      expect(seen[0]?.data).toEqual({ expiredHashToken: currentAsset })
      expect(result).toBe(renewedAsset)
      expect(tokenStore.assetTokenMap.get('asset_a')).toBe(renewedAsset)
    })

    test('refreshes the timestamp token first when it is within the buffer', async () => {
      const tokenStore = useTokenStore('mainId')
      const staleTimestamp = makeJwt(nowSec + 10)
      const freshTimestamp = makeJwt(nowSec + 300)
      const currentAsset = makeJwt(nowSec + 900, 1700000001)
      const renewedAsset = makeJwt(nowSec + 300, 1700000001)
      tokenStore.timestampToken = staleTimestamp
      tokenStore.assetTokenMap.set('asset_a', currentAsset)
      axios.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
        record(seen, config)
        if (config.url === '/post/renew-timestamp-token') return okResponse(config, freshTimestamp)
        if (config.url === '/post/renew-hash-token') return okResponse(config, renewedAsset)
        throw new Error(`unexpected POST ${config.url}`)
      }

      await tokenStore.forceRenewAssetToken('asset_a')

      expect(seen.map((request) => request.url)).toEqual([
        '/post/renew-timestamp-token',
        '/post/renew-hash-token'
      ])
      expect(tokenStore.timestampToken).toBe(freshTimestamp)
      expect(seen[1]?.auth).toBe(`Bearer ${freshTimestamp}`)
      expect(tokenStore.assetTokenMap.get('asset_a')).toBe(renewedAsset)
    })
  })

  describe('renewal failure containment', () => {
    const originalAdapter = axios.defaults.adapter

    beforeEach(() => {
      setActivePinia(createPinia())
      vi.useFakeTimers()
      vi.setSystemTime(NOW_MS)
      vi.spyOn(console, 'error').mockImplementation(() => {})
      axios.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
        const response: AxiosResponse = {
          data: {},
          status: 401,
          statusText: 'Unauthorized',
          headers: {},
          config
        }
        throw new AxiosError(
          'Request failed with status code 401',
          'ERR_BAD_REQUEST',
          config,
          null,
          response
        )
      }
    })

    afterEach(() => {
      axios.defaults.adapter = originalAdapter
      vi.useRealTimers()
      vi.restoreAllMocks()
    })

    test('timestamp renewal failure resolves without throwing and keeps the token', async () => {
      const tokenStore = useTokenStore('mainId')
      const stale = makeJwt(nowSec - 10)
      tokenStore.timestampToken = stale

      await expect(tokenStore.refreshTimestampTokenIfExpired()).resolves.toBeUndefined()

      expect(tokenStore.timestampToken).toBe(stale)
    })

    test('force-renew failures resolve without throwing and keep the map', async () => {
      const tokenStore = useTokenStore('mainId')
      tokenStore.timestampToken = makeJwt(nowSec + 900)
      tokenStore.assetTokenMap.set('asset_a', makeJwt(nowSec + 900, 1700000001))

      await expect(tokenStore.forceRenewTimestampToken()).resolves.toBeUndefined()
      await expect(tokenStore.forceRenewAssetToken('asset_a')).resolves.toBeNull()

      // Escalation belongs to the main axios interceptor contract
      // (see axiosInterceptor.test.ts); nothing here redirects.
      expect(tokenStore.assetTokenMap.get('asset_a')).not.toBeNull()
    })
  })
})
