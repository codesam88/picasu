import axios from 'axios'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useTokenStore } from '@/store/tokenStore'
import { BackendDataParser } from '@/type/schemas'
import { IsolationId, UnifiedData } from '@type/types'

/**
 * Fetch the full metadata record for one asset from `GET /get/metadata/{assetId}`.
 *
 * List rows (get-data) no longer carry tags / EXIF / description / rating
 * (Phase 14 lean payload), so the sidebar, detail view, and edit-dialog prefill
 * fetch them on demand here. Auth mirrors get-data: the prefetch timestamp
 * token as a Bearer header, plus the matching `timestamp` query parameter
 * required by GuardTimestamp.
 */
export async function fetchAssetMetadata(
  assetId: string,
  isolationId: IsolationId
): Promise<UnifiedData | null> {
  const tokenStore = useTokenStore(isolationId)
  const prefetchStore = usePrefetchStore(isolationId)
  const timestamp = prefetchStore.timestamp

  if (timestamp === null) {
    return null
  }

  await tokenStore.refreshTimestampTokenIfExpired()
  const token = tokenStore.timestampToken
  if (token === null) {
    console.error('timestampToken not found for metadata detail fetch')
    return null
  }

  try {
    const response = await axios.get(`/get/metadata/${assetId}`, {
      params: { timestamp },
      headers: { Authorization: `Bearer ${token}` }
    })
    return BackendDataParser.parse(response.data)
  } catch (err) {
    console.error('Failed to fetch asset metadata detail:', err)
    return null
  }
}
