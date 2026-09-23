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
        path: null,
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
        path: null,
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
        path: null,
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

  describe('mergeMetadata', () => {
    const leanRow = {
      type: 'image' as const,
      id: 'content_hash_xyz',
      width: 640,
      height: 480,
      ext: 'jpg',
      size: 1024,
      // Lean list row: no tags/exif/description/rating/flags yet.
      tags: [] as string[],
      exif: {} as Record<string, string>,
      phash: [] as number[],
      thumbhash: null as number[] | null,
      pending: false,
      album: 'album_x' as string | null,
      path: { file: '/photos/a.jpg', modified: 1, scanTime: 2, isTrashed: false } as {
        file: string
        modified: number
        scanTime: number
        isTrashed: boolean
      },
      description: null as string | null,
      isFavorite: false,
      isArchived: false,
      isTrashed: false,
      rating: null as number | null,
      updateAt: 0,
      thumbhashUrl: null as string | null,
      timestamp: 1700000000000,
      assetId: 'asset_m'
    }

    const detail = {
      type: 'image' as const,
      id: 'content_hash_xyz',
      width: 640,
      height: 480,
      ext: 'jpg',
      size: 1024,
      tags: ['sunset', 'nature'],
      exif: { Make: 'Apple' },
      phash: [9, 9],
      thumbhash: null,
      pending: false,
      album: 'album_x',
      path: { file: '/photos/a.jpg', modified: 1, scanTime: 2, isTrashed: false },
      description: 'server description',
      isFavorite: true,
      isArchived: true,
      isTrashed: false,
      rating: 4,
      updateAt: 4242
    }

    test('merges detail metadata into the lean row without touching identity fields', () => {
      const dataStore = useDataStore('mainId')
      dataStore.data.set(0, { ...leanRow, path: { ...leanRow.path } })
      dataStore.assetIdMapData.set('asset_m', 0)

      const merged = dataStore.mergeMetadata(0, detail)

      expect(merged).toBe(true)
      const row = dataStore.data.get(0)
      expect(row).toBeDefined()
      if (row === undefined) return
      // Metadata fields come from the detail payload.
      expect(row.type).toBe('image')
      if (row.type !== 'image') return
      expect(row.tags).toEqual(['sunset', 'nature'])
      expect(row.exif).toEqual({ Make: 'Apple' })
      expect(row.description).toBe('server description')
      expect(row.rating).toBe(4)
      expect(row.isFavorite).toBe(true)
      expect(row.isArchived).toBe(true)
      expect(row.updateAt).toBe(4242)
      expect(row.phash).toEqual([9, 9])
      // Identity fields stay as the list provided them.
      expect(row.assetId).toBe('asset_m')
      expect(row.id).toBe('content_hash_xyz')
      expect(row.width).toBe(640)
      expect(row.album).toBe('album_x')
      expect(row.timestamp).toBe(1700000000000)
    })

    test('returns false for a missing index', () => {
      const dataStore = useDataStore('mainId')
      expect(dataStore.mergeMetadata(7, detail)).toBe(false)
    })

    test('returns false for album rows (albums already carry full metadata)', () => {
      const dataStore = useDataStore('mainId')
      const albumRow = {
        type: 'album' as const,
        id: 'album_a',
        title: null,
        startTime: null,
        endTime: null,
        lastModifiedTime: 0,
        cover: null,
        itemCount: 0,
        tags: [] as string[],
        exif: {} as Record<string, string>,
        thumbhash: null as number[] | null,
        pending: false,
        description: null as string | null,
        isFavorite: false,
        isArchived: false,
        rating: null as number | null,
        updateAt: 0,
        thumbhashUrl: null as string | null,
        timestamp: 1700000000000,
        assetId: 'album_a'
      }
      dataStore.data.set(0, albumRow as unknown as import('@type/types').EnrichedUnifiedData)

      const albumDetail = { ...detail, type: 'album' as const }
      expect(dataStore.mergeMetadata(0, albumDetail as never)).toBe(false)
    })
  })
})
