import { describe, expect, it } from 'vitest'
import { findPhotoAssetId } from './playwright/executeGiven'

// get-data rows are path-primary (backend DataBaseTimestampReturn, camelCase):
// row-level `assetId` is the API identity; the on-disk path lives at
// `abstractData.path.file` (singular). There is no `currentPath` object and no
// row-level `hash` — see frontend/src/type/schemas.ts (FileEntrySchema,
// databaseTimestampSchema) and backend scenario
// locate_same_hash_by_asset_id.yaml (response.json.[0].assetId /
// response.json.[0].abstractData.path.file).
function mediaRow(assetId: string, file: string) {
  return {
    assetId,
    timestamp: 0,
    token: '',
    abstractData: {
      type: 'image',
      id: 'content-hash-not-identity',
      path: { file, modified: 0, scanTime: 0, isTrashed: false }
    }
  }
}

describe('findPhotoAssetId', () => {
  it('returns the row assetId whose path.file ends with the qualified path', () => {
    const data = [
      mediaRow('asset-other', '/data/images/e2e/other/photo.jpg'),
      mediaRow('asset-target', '/data/images/e2e/assigntest/imports/photo.jpg')
    ]
    expect(findPhotoAssetId('assigntest/imports/photo.jpg', data)).toBe('asset-target')
  })

  it('returns null when no row matches the path', () => {
    const data = [mediaRow('asset-1', '/data/images/e2e/a.jpg')]
    expect(findPhotoAssetId('missing/photo.jpg', data)).toBe(null)
  })
})
