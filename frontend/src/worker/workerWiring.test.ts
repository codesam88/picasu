import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { beforeEach, describe, expect, test, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { bindActionDispatch } from 'typesafe-agent-events'
import { fromDataWorker, fromImgWorker } from './workerApi'
import { handleDataWorkerReturn } from './fromDataWorker'
import { handleImgWorker } from './fromImgWorker'
import { useTokenStore } from '@/store/tokenStore'
import { useImgStore } from '@/store/imgStore'
import { thumbnailCacheKey } from './imgCache'

/// Worker-realm sources reference `self` at module scope, so they cannot be
/// imported under node vitest (same reason routes.test.ts checks source
/// instead of importing it). The dispatch wiring is pinned structurally;
/// the main-thread handlers are plain store code and are covered by the
/// runtime round-trip tests below.
const workerDir = import.meta.dirname

function readWorker(file: string): string {
  return readFileSync(resolve(workerDir, file), 'utf8')
}

/// Minimal Worker stand-in: only the listener surface the from* handlers use.
function makeFakeWorker() {
  const listeners = new Map<string, ((e: MessageEvent) => void)[]>()
  return {
    addEventListener: (type: string, fn: (e: MessageEvent) => void) => {
      listeners.set(type, [...(listeners.get(type) ?? []), fn])
    },
    removeEventListener: (type: string, fn: (e: MessageEvent) => void) => {
      listeners.set(
        type,
        (listeners.get(type) ?? []).filter((f) => f !== fn)
      )
    },
    emit: (data: unknown) => {
      for (const fn of listeners.get('message') ?? []) fn({ data } as MessageEvent)
    }
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.restoreAllMocks()
})

describe('worker token-expired wiring', () => {
  test('workerApi declares tokenExpired, no unauthorized', () => {
    const source = readWorker('workerApi.ts')
    expect(source).toContain('tokenExpired')
    expect(source).not.toContain('unauthorized')
  })

  test('data worker dispatches tokenExpired on interceptor 401', () => {
    const source = readWorker('toDataWorker.ts')
    expect(source).toContain('onUnauthorized: postToMainData.tokenExpired')
    expect(source).not.toContain('.unauthorized')
  })

  test('data handler force-renews the timestamp token, never escalates', () => {
    const source = readWorker('fromDataWorker.ts')
    expect(source).toContain('forceRenewTimestampToken')
    expect(source).not.toContain('clearProfile')
    expect(source).not.toContain('redirectionToLogin')
    expect(source).not.toContain('unauthorized')
  })

  test('image worker reports tokenExpired with assetId from the 401 catch blocks', () => {
    const source = readWorker('toImgWorker.ts')
    expect(source).toContain('postToMainImg.tokenExpired({ assetId: event.assetId })')
    expect(source).toMatch(/axios\.isAxiosError\(error\) && error\.response\?\.status === 401/)
    expect(source).not.toContain('.unauthorized')
  })

  test('image handler force-renews the asset token, never escalates', () => {
    const source = readWorker('fromImgWorker.ts')
    expect(source).toContain('forceRenewAssetToken')
    expect(source).not.toContain('clearProfile')
    expect(source).not.toContain('redirectionToLogin')
    expect(source).not.toContain('unauthorized')
  })

  test('grid thumbnails are re-encoded below maximum quality', () => {
    const source = readWorker('toImgWorker.ts')
    expect(source).toMatch(/THUMBNAIL_QUALITY = 0\.8/)
    expect(source).toContain('quality: THUMBNAIL_QUALITY')
    expect(source).not.toMatch(/quality:\s*1[\s,}]/)
  })

  test('thumbnail cache lookup wraps the decode instead of following it', () => {
    const source = readWorker('toImgWorker.ts')
    const encodeCall = source.indexOf('getOrEncodeThumbnail(resizedBlobCache')
    const decodeCall = source.indexOf('createImageBitmap(sourceBlob)')
    expect(encodeCall).toBeGreaterThan(-1)
    expect(decodeCall).toBeGreaterThan(encodeCall)
    expect(source).toContain('resizedBlobCache.deleteWhere')
  })

  test('data tokenExpired round-trips to forceRenewTimestampToken', () => {
    const fake = makeFakeWorker()
    handleDataWorkerReturn(fake as unknown as Worker, 'mainId')
    const post = bindActionDispatch(fromDataWorker, (action) => {
      fake.emit(action)
    })

    const renewSpy = vi
      .spyOn(useTokenStore('mainId'), 'forceRenewTimestampToken')
      .mockResolvedValue(undefined)

    post.tokenExpired()

    expect(renewSpy).toHaveBeenCalledTimes(1)
  })

  test('image tokenExpired round-trips to forceRenewAssetToken with the assetId', () => {
    const fake = makeFakeWorker()
    handleImgWorker(fake as unknown as Worker, 'mainId')
    const post = bindActionDispatch(fromImgWorker, (action) => {
      fake.emit(action)
    })

    const renewSpy = vi
      .spyOn(useTokenStore('mainId'), 'forceRenewAssetToken')
      .mockResolvedValue('new-token')

    post.tokenExpired({ assetId: 'asset_1' })

    expect(renewSpy).toHaveBeenCalledTimes(1)
    expect(renewSpy).toHaveBeenCalledWith('asset_1')
  })
})

describe('thumbnail refresh reuse wiring', () => {
  const keyFor = (assetId: string, width = 320) =>
    thumbnailCacheKey({
      assetId,
      width,
      height: 240,
      devicePixelRatio: 2,
      albumMode: false
    })

  test('the worker posts the output key and asset identity, not the tile index', () => {
    const source = readWorker('toImgWorker.ts')
    expect(source).toContain('cacheKey: resizedKey')
    expect(source).toContain('assetId: event.assetId')
    expect(source).toContain('albumMode')
    expect(source).not.toContain('smallImageProcessed({ index: event.index')
  })

  test('the main thread keys tile URLs by asset identity and output key', () => {
    const source = readFileSync(
      resolve(workerDir, '../components/Buffer/FunctionalComponent/SmallImageContainer.ts'),
      'utf8'
    )
    expect(source).toContain('thumbnailCacheKey')
    expect(source).toContain('getImgUrl(serving.assetId')
    expect(source).not.toContain('imgUrl.get(props.index)')
  })

  test('refresh paths do not blanket-clear tile URLs', () => {
    const prefetch = readFileSync(resolve(workerDir, '../script/hook/usePrefetch.ts'), 'utf8')
    const gallery = readFileSync(resolve(workerDir, '../components/Gallery/Gallery.vue'), 'utf8')
    // Both paths still drop the per-index row/queue state so misses re-queue...
    expect(prefetch).toContain('useQueueStore(isolationId).clearAll()')
    expect(gallery).toContain('queueStore.clearAll()')
    // ...but neither drops the asset-keyed tile URLs.
    expect(prefetch).not.toContain('imgStore.clearForResize')
    expect(gallery).not.toContain('imgStore.clearForResize')
  })

  test('purge still drops the blob, every resized variant, and the disk entry', () => {
    const source = readWorker('toImgWorker.ts')
    expect(source).toContain('blobCache.delete(assetId)')
    expect(source).toContain('resizedBlobCache.deleteWhere((key) => key.startsWith(`${assetId}|`))')
    expect(source).toContain('cache.delete(imgCacheKey(assetId))')
  })

  test('a processed thumbnail round-trips into reuse by asset and key', () => {
    const fake = makeFakeWorker()
    handleImgWorker(fake as unknown as Worker, 'mainId')
    const post = bindActionDispatch(fromImgWorker, (action) => {
      fake.emit(action)
    })
    const store = useImgStore('mainId')

    post.smallImageProcessed({
      assetId: 'asset_1',
      cacheKey: keyFor('asset_1'),
      albumMode: false,
      url: 'blob:thumb'
    })

    expect(store.getImgUrl('asset_1', false, keyFor('asset_1'))).toBe('blob:thumb')
    expect(store.getImgUrl('asset_1', false, keyFor('asset_1', 640))).toBeUndefined()
    expect(store.getImgUrl('asset_2', false, keyFor('asset_2'))).toBeUndefined()
  })
})
