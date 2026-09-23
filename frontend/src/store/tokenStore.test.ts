import { describe, expect, test, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useTokenStore } from './tokenStore'

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
})
