import { IsolationId, ResolvedShare } from '@type/types'
import { defineStore } from 'pinia'
import { storeShareInfo, clearShareInfo } from '@/db/db'

export const useShareStore = (isolationId: IsolationId) =>
  defineStore('shareStore' + isolationId, {
    state: (): {
      albumId: null | string
      shareId: null | string
      password: null | string
      isAuthFailed: boolean
      isLinkExpired: boolean
      resolvedShare: null | ResolvedShare
    } => ({
      albumId: null,
      shareId: null,
      password: null,
      isAuthFailed: false,
      isLinkExpired: false,
      resolvedShare: null
    }),
    actions: {
      async syncShareInfoToIndexedDB() {
        // eslint-disable-next-line @typescript-eslint/strict-boolean-expressions
        if (this.albumId && this.shareId) {
          await storeShareInfo({
            albumId: this.albumId,
            shareId: this.shareId,
            password: this.password
          })
        }
      },
      async clearShareInfoFromIndexedDB() {
        // eslint-disable-next-line @typescript-eslint/strict-boolean-expressions
        if (this.albumId && this.shareId) {
          await clearShareInfo(this.albumId, this.shareId)
        }
      }
    }
  })()
