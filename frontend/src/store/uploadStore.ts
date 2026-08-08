// src/store/uploadStore.ts
import { defineStore } from 'pinia'
import axios, { type AxiosProgressEvent } from 'axios'
import { useMessageStore } from './messageStore'
import { useModalStore } from './modalStore'
import { errorDisplay } from '@/script/utils/errorDisplay'
import { IsolationId } from '@type/types'

/** Build the upload endpoint URL with the target album (if any) and the
 * `auto_rename` choice. Kept pure for unit testing. */
export function buildUploadUrl(albumId: string | undefined, autoRename: boolean): string {
  const params: string[] = []
  if (albumId !== undefined) {
    params.push(`presigned_album_id_opt=${encodeURIComponent(albumId)}`)
  }
  params.push(`auto_rename=${autoRename}`)
  return params.length > 0 ? `/upload?${params.join('&')}` : '/upload'
}

export const useUploadStore = (isolationId: IsolationId) =>
  defineStore('uploadStore' + isolationId, {
    state: () => ({
      status: 'Canceled',
      total: undefined as number | undefined,
      loaded: undefined as number | undefined,
      startTime: undefined as number | undefined,
      abortController: null as AbortController | null,
      // Files picked but not yet confirmed via the pre-upload options dialog.
      pendingFiles: [] as File[],
      pendingAlbumId: undefined as string | undefined,
      autoRename: true
    }),

    getters: {
      percentComplete: (state): number =>
        state.total !== undefined && state.loaded !== undefined && state.total > 0
          ? Math.floor((state.loaded / state.total) * 100)
          : 0,

      elapsedTime: (state): number =>
        state.startTime !== undefined ? (Date.now() - state.startTime) / 1000 : 0,

      uploadSpeed(): number {
        const elapsed = this.elapsedTime
        return elapsed > 0 && this.loaded !== undefined ? this.loaded / elapsed : 0 // bytes/sec
      },

      remainingTime(): number {
        const speed = this.uploadSpeed
        if (speed > 0 && this.total !== undefined && this.loaded !== undefined) {
          return (this.total - this.loaded) / speed // seconds
        }
        return 0
      }
    },

    actions: {
      triggerFileInput(albumId: string | undefined): void {
        const fileInput = document.createElement('input')
        fileInput.type = 'file'
        fileInput.multiple = true
        fileInput.style.display = 'none'

        const handleChange = (event: Event): void => {
          const target = event.target as HTMLInputElement
          const files = target.files
          try {
            if (files && files.length > 0) {
              this.prepareUpload([...files], albumId)
            }
          } finally {
            document.body.removeChild(fileInput)
          }
        }

        fileInput.addEventListener('change', handleChange, { once: true })
        document.body.appendChild(fileInput)
        fileInput.click()
      },

      /** Stage files and show the pre-upload options dialog. Upload happens
       * only after the user confirms there. */
      prepareUpload(files: File[], albumId: string | undefined): void {
        const modalStore = useModalStore('mainId')
        this.pendingFiles = files
        this.pendingAlbumId = albumId
        modalStore.showUploadOptionsModal = true
      },

      /** Confirm the staged upload from the options dialog. */
      async confirmUpload(): Promise<void> {
        const files = [...this.pendingFiles]
        const albumId = this.pendingAlbumId
        const autoRename = this.autoRename
        const modalStore = useModalStore('mainId')
        modalStore.showUploadOptionsModal = false
        await this.fileUpload(files, albumId, autoRename)
      },

      /** Discard staged files and close the options dialog. */
      cancelUploadOptions(): void {
        const modalStore = useModalStore('mainId')
        this.pendingFiles = []
        this.pendingAlbumId = undefined
        modalStore.showUploadOptionsModal = false
      },

      async fileUpload(
        files: File[],
        albumId: string | undefined,
        autoRename: boolean
      ): Promise<void> {
        const modalStore = useModalStore('mainId')
        const messageStore = useMessageStore('mainId')

        this.status = 'Uploading'
        modalStore.showUploadModal = true

        const formData = new FormData()
        for (const file of files) {
          formData.append('file', file)
          formData.append('lastModified', String(file.lastModified))
        }

        const uploadUrl = buildUploadUrl(albumId, autoRename)

        const abortController = new AbortController()
        this.abortController = abortController
        this.total = this.loaded = 0
        this.startTime = Date.now()

        try {
          await axios.post(uploadUrl, formData, {
            headers: { 'Content-Type': 'multipart/form-data' },
            signal: abortController.signal,
            onUploadProgress: (e: AxiosProgressEvent) => {
              if (e.total !== undefined) {
                this.total = e.total
                // Axios types say loaded can be undefined
                if (typeof e.loaded === 'number') {
                  this.loaded = e.loaded
                }
                if (this.loaded !== undefined && this.total === this.loaded) {
                  this.status = 'Processing'
                }
              }
            }
          })

          this.status = 'Completed'
          messageStore.success('Files uploaded successfully')
        } catch (err) {
          this.status = 'Canceled'
          messageStore.error(errorDisplay(err))
        }
      },

      cancelUpload(): void {
        if (this.abortController) {
          this.abortController.abort()
          this.status = 'Canceled'
        }
      }
    }
  })()
