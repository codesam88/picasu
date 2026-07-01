import { IsolationId } from '@type/types'
import { defineStore } from 'pinia'

export const useModalStore = (isolationId: IsolationId) =>
  defineStore('modalStore' + isolationId, {
    state: (): {
      showEditTagsModal: boolean
      showBatchEditTagsModal: boolean
      showAssignAlbumModal: boolean
      assignAlbumBatch: boolean
      showUploadModal: boolean
      showIsolatedHomeModal: boolean
      showHomeTempModal: boolean
      showShareModal: boolean
      showEditShareModal: boolean
      showDeleteShareModal: boolean
      showShareLoginModal: boolean
      showAlbumInfoModal: boolean
    } => ({
      showEditTagsModal: false,
      showBatchEditTagsModal: false,
      showAssignAlbumModal: false,
      assignAlbumBatch: false,
      showUploadModal: false,
      showIsolatedHomeModal: false,
      showHomeTempModal: false,
      showShareModal: false,
      showEditShareModal: false,
      showDeleteShareModal: false,
      showShareLoginModal: false,
      showAlbumInfoModal: false
    }),
    actions: {}
  })()
