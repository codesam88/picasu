import { thumbHashToDataURL } from 'thumbhash'
import { EnrichedUnifiedData, UnifiedData } from '@type/types'

/**
 * Enriches data with a thumbhash URL.
 * Backend data is already flattened by Zod transformation.
 * Requires assetId and timestamp to be present on the input data.
 */
export function enrichWithThumbhash(
  data: UnifiedData & { assetId: string; timestamp: number }
): EnrichedUnifiedData {
  const thumbhashUrl = data.thumbhash ? thumbHashToDataURL(data.thumbhash) : null
  return { ...data, thumbhashUrl }
}

/**
 * Returns the appropriate filename/title for display.
 */
export function getFilename(data: UnifiedData): string {
  if (data.type === 'image' || data.type === 'video') {
    return data.alias[0]?.file.split('/').pop() ?? ''
  }
  return data.title ?? ''
}
