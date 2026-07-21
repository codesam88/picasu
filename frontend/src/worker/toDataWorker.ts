import { rowSchema, databaseTimestampSchema } from '@type/schemas'
import { DisplayElement, FetchDataMethod, UnifiedData } from '@type/types'
import { batchNumber } from '@/type/constants'
import { enrichWithThumbhash } from '@utils/createData'

import axios from 'axios'

import { bindActionDispatch, createHandler } from 'typesafe-agent-events'
import { fromDataWorker, toDataWorker } from './workerApi'
import { z } from 'zod'
import { setupWorkerAxiosInterceptor } from './workerAxiosInterceptor'

const shouldProcessBatch: number[] = []
const fetchedRowData = new Map<number, { displayElements: DisplayElement[]; start: number }>()
const postToMainData = bindActionDispatch(fromDataWorker, self.postMessage.bind(self))
const workerAxios = axios.create()

setupWorkerAxiosInterceptor(workerAxios, postToMainData.notification)

self.addEventListener('message', (e) => {
  const handler = createHandler<typeof toDataWorker>({
    fetchData: async (payload) => {
      const { fetchMethod, batch, timestamp, timestampToken } = payload

      // if there are too many batch are processed then try to terminate the oldest request
      if (fetchMethod === 'batch') {
        if (shouldProcessBatch.length >= 6) {
          shouldProcessBatch.shift()
        }
        shouldProcessBatch.push(batch)
      }

      const { result, startIndex, endIndex } = await fetchData(
        fetchMethod,
        batch,
        timestamp,
        timestampToken
      )

      if (result.size > 0) {
        const indices = Array.from({ length: endIndex - startIndex }, (_, i) => startIndex + i)

        const slicedDataArray: {
          index: number
          data: UnifiedData & { thumbhashUrl: string | null; timestamp: number }
          hashToken: string
        }[] = []
        for (const index of indices) {
          const getData = result.get(index)
          if (getData !== undefined) {
            slicedDataArray.push({
              index,
              data: getData.abstractData,
              hashToken: getData.hashToken
            })
          }
        }

        postToMainData.returnData({ batch: batch, slicedDataArray: slicedDataArray })
      }
    },
    fetchRow: async (payload) => {
      const { index, timestamp, windowWidth, timestampToken } = payload

      const { displayElements, start } = await fetchRow(
        index,
        timestamp,
        windowWidth,
        timestampToken
      )

      postToMainData.fetchRowReturn({
        chunkIndex: index,
        displayElements,
        start,
        timestamp
      })
    }
  })
  handler(e.data as ReturnType<(typeof toDataWorker)[keyof typeof toDataWorker]>)
})

/**
/**
 * Fetches a batch of data based on the provided batch index and timestamp.
 * Processes the fetched data into UnifiedData instances and accumulates them into a map.
 *
 * @param batchIndex - The index of the batch to fetch.
 * @param timestamp - The timestamp associated with the data fetch.
 * @returns A promise that resolves to a Map of data entries keyed by their index,
 *          or an object containing an error message and a warning flag if an error occurs.
 */
async function fetchData(
  fetchMethod: FetchDataMethod,
  index: number,
  timestamp: number,
  timestampToken: string
): Promise<{
  result: Map<
    number,
    {
      abstractData: UnifiedData & { thumbhashUrl: string | null; timestamp: number }
      hashToken: string
    }
  >
  startIndex: number
  endIndex: number
}> {
  let start: number
  let end: number
  switch (fetchMethod) {
    case 'batch': {
      const batchIndex = index
      start = batchIndex * batchNumber
      end = (batchIndex + 1) * batchNumber
      break
    }
    case 'single': {
      start = index
      end = index + 1
      break
    }
  }

  const fetchUrl = `/get/get-data?timestamp=${timestamp}&start=${start}&end=${end}`

  const response = await workerAxios.get(fetchUrl, {
    headers: {
      Authorization: `Bearer ${timestampToken}`
    }
  })
  const databaseTimestampArray = z.array(databaseTimestampSchema).parse(response.data)

  const data = new Map<
    number,
    {
      abstractData: UnifiedData & { thumbhashUrl: string | null; timestamp: number }
      hashToken: string
    }
  >()

  for (let i = 0; i < databaseTimestampArray.length; i++) {
    // Determine the current batch index based on the fetch method
    const currentBatchIndex = fetchMethod === 'batch' ? Math.floor(start / batchNumber) : index

    if (fetchMethod === 'batch' && !shouldProcessBatch.includes(currentBatchIndex)) {
      break // Stop processing further if the batch should no longer be processed
    }

    const item = databaseTimestampArray[i]
    const key = start + i

    if (item === undefined) {
      console.error(
        `Error processing item at ${fetchMethod === 'batch' ? 'batchIndex' : 'index'}: ${
          fetchMethod === 'batch' ? index : index
        }, ` + `batchNumber: ${batchNumber}, index: ${i}. Item is undefined.`
      )
      continue
    }

    const dataWithCorrectTimestamp = {
      ...item.abstractData,
      timestamp: item.timestamp
    }
    const enrichedData = enrichWithThumbhash(dataWithCorrectTimestamp)
    data.set(key, { abstractData: enrichedData, hashToken: item.token })

    if (i % 100 === 0) {
      // Yield after every 100 items
      await new Promise((resolve) => setTimeout(resolve, 0))
    }
  }

  return { result: data, startIndex: start, endIndex: end }
}

/**
 * Fetches a specific row of display elements based on the provided index and timestamp.
 * Returns a flat array of DisplayElements without layout computation.
 *
 * @param index - The index of the row to fetch.
 * @param timestamp - The timestamp associated with the data fetch.
 * @param windowWidth - The width of the window/container.
 * @param timestampToken - The authentication token.
 * @returns A promise that resolves to an object containing the display elements and start index.
 */
async function fetchRow(
  index: number,
  timestamp: number,
  windowWidth: number,
  timestampToken: string
): Promise<{ displayElements: DisplayElement[]; start: number }> {
  const cached = fetchedRowData.get(index)

  if (cached !== undefined) {
    return cached
  }

  const response = await workerAxios.get<{ displayElements: DisplayElement[]; start: number }>(
    `/get/get-rows?index=${index}&timestamp=${timestamp}&window_width=${Math.round(windowWidth)}`,
    {
      headers: {
        Authorization: `Bearer ${timestampToken}`
      }
    }
  )

  const row = rowSchema.parse(response.data)
  const result = {
    displayElements: row.displayElements,
    start: row.start
  }

  fetchedRowData.set(index, result)
  return result
}
