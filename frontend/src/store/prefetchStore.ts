import { IsolationId } from '@type/types'
import { defineStore } from 'pinia'

export const usePrefetchStore = (isolationId: IsolationId) =>
  defineStore('prefetchStore' + isolationId, {
    state: (): {
      windowWidth: number
      timestamp: number | null
      dataLength: number
      locateTo: number | null
      updateVisibleRowTrigger: boolean
      updateFetchRowTrigger: boolean
      filterBasicString: string | null
    } => ({
      windowWidth: 0,
      timestamp: null,
      dataLength: 0,
      locateTo: null,
      updateVisibleRowTrigger: false,
      updateFetchRowTrigger: false,
      filterBasicString: null
    }),
    actions: {
      calculateLength(dataLength: number) {
        this.dataLength = dataLength
      },
      clearAll() {
        this.timestamp = null
        this.dataLength = 0
        this.locateTo = null
        this.updateVisibleRowTrigger = !this.updateVisibleRowTrigger
      }
    }
  })()
