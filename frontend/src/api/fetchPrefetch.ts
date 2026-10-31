import axios from 'axios'
import { Prefetch, PrefetchReturn } from '@type/types'
import { prefetchReturnSchema } from '@type/schemas'

export async function prefetch(
  filterJsonString: string | null,
  _priorityId: string | undefined = 'default',
  _reverse: string | undefined = 'false',
  locate: null | string = null,
  includePersonal: boolean = false
): Promise<PrefetchReturn> {
  const params: string[] = []
  if (locate !== null) params.push(`locate=${locate}`)
  if (includePersonal) params.push('include_personal=true')
  const fetchUrl = `/get/prefetch?${params.join('&')}`

  const axiosResponse = await axios.post<Prefetch>(fetchUrl, filterJsonString, {
    headers: {
      'Content-Type': 'application/json'
    }
  })

  const prefetchReturn = prefetchReturnSchema.parse(axiosResponse.data)

  return prefetchReturn
}
