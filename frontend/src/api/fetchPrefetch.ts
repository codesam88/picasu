import axios from 'axios'
import { Prefetch, PrefetchReturn } from '@type/types'
import { prefetchReturnSchema } from '@type/schemas'

export async function prefetch(
  filterJsonString: string | null,
  _priorityId: string | undefined = 'default',
  _reverse: string | undefined = 'false',
  locate: null | string = null
): Promise<PrefetchReturn> {
  const fetchUrl = `/get/prefetch?${locate !== null ? `locate=${locate}` : ''}`

  const axiosResponse = await axios.post<Prefetch>(fetchUrl, filterJsonString, {
    headers: {
      'Content-Type': 'application/json'
    }
  })

  const prefetchReturn = prefetchReturnSchema.parse(axiosResponse.data)

  return prefetchReturn
}
