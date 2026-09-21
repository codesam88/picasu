import { describe, expect, test, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useDataStore } from './dataStore'

describe('dataStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  describe('assetIdMapData', () => {
    test('same-hash rows with different assetIds are distinguishable via assetIdMapData', () => {
      const dataStore = useDataStore('mainId')

      const baseData = {
        type: 'image' as const,
        id: 'same_hash_abc',
        width: 100,
        height: 100,
        ext: 'jpg',
        size: 1024,
        tags: [],
        exif: {},
        phash: [],
        thumbhash: null,
        pending: false,
        album: null as string | null,
        alias: [],
        description: null,
        isFavorite: false,
        isArchived: false,
        isTrashed: false,
        rating: null,
        updateAt: 0,
        thumbhashUrl: null,
        timestamp: 1700000000000,
        assetId: 'placeholder'
      }

      // Two items with same content hash but different asset IDs
      const item1Data = { ...baseData, album: 'album_a', assetId: 'asset_a' }
      const item2Data = { ...baseData, album: 'album_b', assetId: 'asset_b' }

      // Populate via assetIdMapData only (no hashMapData)
      dataStore.data.set(0, item1Data)
      dataStore.assetIdMapData.set('asset_a', 0)

      dataStore.data.set(1, item2Data)
      dataStore.assetIdMapData.set('asset_b', 1)

      // assetIdMapData has both items with distinct indices
      const index1 = dataStore.assetIdMapData.get('asset_a')
      const index2 = dataStore.assetIdMapData.get('asset_b')

      expect(index1).toBe(0)
      expect(index2).toBe(1)
      expect(index1).not.toBe(index2)

      // Verify the data is correct
      const retrieved1 = index1 !== undefined ? dataStore.data.get(index1) : undefined
      const retrieved2 = index2 !== undefined ? dataStore.data.get(index2) : undefined
      expect(retrieved1?.type).toBe('image')
      expect(retrieved2?.type).toBe('image')
      if (retrieved1?.type === 'image') {
        expect(retrieved1.album).toBe('album_a')
        expect(retrieved1.assetId).toBe('asset_a')
      }
      if (retrieved2?.type === 'image') {
        expect(retrieved2.album).toBe('album_b')
        expect(retrieved2.assetId).toBe('asset_b')
      }
    })

    test('assetIdMapData is the only identity map', () => {
      const dataStore = useDataStore('mainId')

      const itemData = {
        type: 'image' as const,
        id: 'content_hash_xyz',
        width: 100,
        height: 100,
        ext: 'jpg',
        size: 1024,
        tags: [],
        exif: {},
        phash: [],
        thumbhash: null,
        pending: false,
        album: null as string | null,
        alias: [],
        description: null,
        isFavorite: false,
        isArchived: false,
        isTrashed: false,
        rating: null,
        updateAt: 0,
        thumbhashUrl: null,
        timestamp: 1700000000000,
        assetId: 'asset_xyz'
      }

      // Store by assetId
      dataStore.data.set(0, itemData)
      dataStore.assetIdMapData.set('asset_xyz', 0)

      // Should be retrievable by assetId
      const index = dataStore.assetIdMapData.get('asset_xyz')
      expect(index).toBe(0)

      // Should be retrievable by assetId
      expect(dataStore.assetIdMapData.size).toBe(1)
    })

    test('clearAll clears all maps', () => {
      const dataStore = useDataStore('mainId')

      dataStore.data.set(0, {
        type: 'image',
        id: 'hash1',
        width: 100,
        height: 100,
        ext: 'jpg',
        size: 1024,
        tags: [],
        exif: {},
        phash: [],
        thumbhash: null,
        pending: false,
        album: null,
        alias: [],
        description: null,
        isFavorite: false,
        isArchived: false,
        isTrashed: false,
        rating: null,
        updateAt: 0,
        thumbhashUrl: null,
        timestamp: 1700000000000,
        assetId: 'asset1'
      })
      dataStore.assetIdMapData.set('asset1', 0)
      dataStore.batchFetched.set(0, true)

      expect(dataStore.data.size).toBe(1)
      expect(dataStore.assetIdMapData.size).toBe(1)
      expect(dataStore.batchFetched.size).toBe(1)

      dataStore.clearAll()

      expect(dataStore.data.size).toBe(0)
      expect(dataStore.assetIdMapData.size).toBe(0)
      expect(dataStore.batchFetched.size).toBe(0)
    })
  })
})
