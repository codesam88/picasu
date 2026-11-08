import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { MockInstance } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { thumbnailCacheKey } from '@/worker/imgCache'
import { thumbnailSlot, useImgStore } from './imgStore'

const key = (assetId: string, width = 320, albumMode = false) =>
  thumbnailCacheKey({
    assetId,
    width,
    height: 240,
    devicePixelRatio: 2,
    albumMode
  })

let revokeSpy: MockInstance<typeof URL.revokeObjectURL>

beforeEach(() => {
  setActivePinia(createPinia())
  revokeSpy = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined)
})

afterEach(() => {
  revokeSpy.mockRestore()
})

describe('imgStore thumbnail URLs', () => {
  test('same asset and output key survive a refresh unchanged', () => {
    const store = useImgStore('mainId')
    const cacheKey = key('asset_a')

    store.setImgUrl('asset_a', false, cacheKey, 'blob:a')
    // A refresh remaps indices but leaves the asset and output box alone.
    expect(store.getImgUrl('asset_a', false, cacheKey)).toBe('blob:a')
    expect(revokeSpy).not.toHaveBeenCalled()
  })

  test('an asset that changes at an index does not resolve the old asset URL', () => {
    const store = useImgStore('mainId')
    store.setImgUrl('asset_old', false, key('asset_old'), 'blob:old')

    // Index 0 now holds a different asset: lookup misses and re-queues.
    expect(store.getImgUrl('asset_new', false, key('asset_new'))).toBeUndefined()
    // The old asset still resolves under its own identity if it reappears.
    expect(store.getImgUrl('asset_old', false, key('asset_old'))).toBe('blob:old')
  })

  test('a changed output box misses and the replacement revokes the old URL', () => {
    const store = useImgStore('mainId')
    const small = key('asset_a', 320)
    const large = key('asset_a', 640)

    store.setImgUrl('asset_a', false, small, 'blob:small')
    expect(store.getImgUrl('asset_a', false, large)).toBeUndefined()

    store.setImgUrl('asset_a', false, large, 'blob:large')

    expect(store.getImgUrl('asset_a', false, large)).toBe('blob:large')
    expect(store.getImgUrl('asset_a', false, small)).toBeUndefined()
    expect(revokeSpy).toHaveBeenCalledWith('blob:small')
  })

  test('duplicate responses for a live key keep one URL and revoke the duplicate', () => {
    const store = useImgStore('mainId')
    const cacheKey = key('asset_a')

    store.setImgUrl('asset_a', false, cacheKey, 'blob:first')
    store.setImgUrl('asset_a', false, cacheKey, 'blob:second')

    expect(store.getImgUrl('asset_a', false, cacheKey)).toBe('blob:first')
    expect(revokeSpy).toHaveBeenCalledWith('blob:second')
  })

  test('contain and cover tiles of the same asset keep independent URLs', () => {
    const store = useImgStore('mainId')
    expect(thumbnailSlot('asset_a', false)).not.toBe(thumbnailSlot('asset_a', true))

    store.setImgUrl('asset_a', false, key('asset_a'), 'blob:contain')
    store.setImgUrl('asset_a', true, key('asset_a', 320, true), 'blob:cover')

    expect(store.getImgUrl('asset_a', false, key('asset_a'))).toBe('blob:contain')
    expect(store.getImgUrl('asset_a', true, key('asset_a', 320, true))).toBe('blob:cover')
    expect(revokeSpy).not.toHaveBeenCalled()
  })

  test('clearAll revokes every live thumbnail and original URL', () => {
    const store = useImgStore('mainId')
    store.setImgUrl('asset_a', false, key('asset_a'), 'blob:a')
    store.setImgUrl('asset_b', false, key('asset_b'), 'blob:b')
    store.setImgOriginal(0, 'blob:original')

    store.clearAll()

    expect(store.imgUrl.size).toBe(0)
    expect(store.imgOriginal.size).toBe(0)
    expect(revokeSpy).toHaveBeenCalledTimes(3)
    expect(revokeSpy).toHaveBeenCalledWith('blob:a')
    expect(revokeSpy).toHaveBeenCalledWith('blob:b')
    expect(revokeSpy).toHaveBeenCalledWith('blob:original')
  })

  test('replacing an original frame revokes the previous URL', () => {
    const store = useImgStore('mainId')
    store.setImgOriginal(0, 'blob:first')
    store.setImgOriginal(0, 'blob:second')

    expect(store.imgOriginal.get(0)).toBe('blob:second')
    expect(revokeSpy).toHaveBeenCalledWith('blob:first')
  })
})
