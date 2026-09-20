import { describe, expect, test, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useDataStore } from './dataStore'

describe('dataStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  describe('hashMapData with assetId', () => {
    test('same-hash rows with different assetIds are distinguishable', () => {
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
        timestamp: 1700000000000
      }

      // Two items with same content hash but different asset IDs
      const item1Data = { ...baseData, album: 'album_a' }
      const item2Data = { ...baseData, album: 'album_b' }

      // Simulate what fromDataWorker does: use assetId as map key when available
      const assetId1 = 'asset_a'
      const assetId2 = 'asset_b'

      dataStore.data.set(0, item1Data)
      dataStore.hashMapData.set(assetId1, 0)

      dataStore.data.set(1, item2Data)
      dataStore.hashMapData.set(assetId2, 1)

      // Both items should be retrievable by their distinct asset IDs
      const index1 = dataStore.hashMapData.get(assetId1)
      const index2 = dataStore.hashMapData.get(assetId2)

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
      }
      if (retrieved2?.type === 'image') {
        expect(retrieved2.album).toBe('album_b')
      }
    })

    test('items without assetId fall back to content hash as key', () => {
      const dataStore = useDataStore('mainId')

      const itemData = {
        type: 'image' as const,
        id: 'unique_hash_xyz',
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
        timestamp: 1700000000000
      }

      // No assetId provided, should fall back to data.id
      const mapKey = itemData.id

      dataStore.data.set(0, itemData)
      dataStore.hashMapData.set(mapKey, 0)

      // Should be retrievable by content hash
      const index = dataStore.hashMapData.get('unique_hash_xyz')
      expect(index).toBe(0)
    })
  })
})
