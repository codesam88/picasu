import { describe, expect, test, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useEditStore } from './editStore'

describe('editStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  describe('asset-based identity', () => {
    test('two same-hash assets are tracked independently by assetId', () => {
      const editStore = useEditStore('mainId')

      editStore.addRegenerate('asset_a')
      editStore.addRegenerate('asset_b')

      expect(editStore.hasRegenerate('asset_a')).toBe(true)
      expect(editStore.hasRegenerate('asset_b')).toBe(true)

      editStore.removeRegenerate('asset_a')

      expect(editStore.hasRegenerate('asset_a')).toBe(false)
      expect(editStore.hasRegenerate('asset_b')).toBe(true)
    })

    test('rotation counts are independent per assetId', () => {
      const editStore = useEditStore('mainId')

      editStore.incrementRotation('asset_a')
      editStore.incrementRotation('asset_a')
      editStore.incrementRotation('asset_b')

      expect(editStore.rotationCounts.get('asset_a')).toBe(2)
      expect(editStore.rotationCounts.get('asset_b')).toBe(1)
    })
  })
})
