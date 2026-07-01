import { IsolationId } from '@type/types'
import { defineStore } from 'pinia'

export const useRerenderStore = (isolationId: IsolationId) =>
  defineStore('rerenderStore' + isolationId, {
    state: (): {
      galleryKey: boolean
    } => ({
      galleryKey: false
    }),
    actions: {
      rerenderGallery() {
        this.galleryKey = !this.galleryKey
      }
    }
  })()
