import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { beforeEach, describe, expect, test, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { bindActionDispatch } from 'typesafe-agent-events'
import { fromDataWorker, fromImgWorker } from './workerApi'
import { handleDataWorkerReturn } from './fromDataWorker'
import { handleImgWorker } from './fromImgWorker'
import { useTokenStore } from '@/store/tokenStore'

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
  const listeners = new Map<string, Array<(e: MessageEvent) => void>>()
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
