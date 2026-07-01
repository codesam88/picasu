import { IsolationId } from '@type/types'
import { defineStore } from 'pinia'

// Dialog-visibility flags only — excludes `assignAlbumBatch`, which is a mode
// toggle rather than a dialog, and `showIsolatedHomeModal`, which is dead
// state left over from the removed isolation system.
const dialogKeys = [
  'showEditTagsModal',
  'showBatchEditTagsModal',
  'showAssignAlbumModal',
  'showUploadModal',
  'showShareModal',
  'showEditShareModal',
  'showDeleteShareModal',
  'showShareLoginModal',
  'showAlbumInfoModal'
] as const

export const useModalStore = (isolationId: IsolationId) =>
  defineStore('modalStore' + isolationId, {
    state: (): {
      showEditTagsModal: boolean
      showBatchEditTagsModal: boolean
      showAssignAlbumModal: boolean
      assignAlbumBatch: boolean
      showUploadModal: boolean
      showIsolatedHomeModal: boolean
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
      showShareModal: false,
      showEditShareModal: false,
      showDeleteShareModal: false,
      showShareLoginModal: false,
      showAlbumInfoModal: false
    }),
    getters: {
      hasOpenDialog: (state) => dialogKeys.some((key) => state[key])
    },
    actions: {
      // Closes whichever dialog is currently open. Used by the global Escape
      // handler, which must close a dialog rather than falling through to
      // back-navigation or edit-mode-exit.
      closeOpenDialog() {
        for (const key of dialogKeys) this[key] = false
      }
    }
  })()
