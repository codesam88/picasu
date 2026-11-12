// src/store/uploadStore.ts
import { defineStore } from 'pinia'
import axios, { type AxiosProgressEvent } from 'axios'
import { useMessageStore } from './messageStore'
import { useModalStore } from './modalStore'
import { errorDisplay } from '@/script/utils/errorDisplay'
import { refreshGalleryAfterMutation } from '@/script/hook/usePrefetch'
import { prefetch } from '@/api/fetchPrefetch'
import { IsolationId } from '@type/types'

/** Build the upload endpoint URL with the target album (if any) and the
 * `auto_rename` choice. Kept pure for unit testing. */
export function buildUploadUrl(albumId: string | undefined, autoRename: boolean): string {
  const params: string[] = []
  if (albumId !== undefined) {
    params.push(`presigned_album_id_opt=${encodeURIComponent(albumId)}`)
  }
  params.push(`auto_rename=${autoRename}`)
  return params.length > 0 ? `/post/upload?${params.join('&')}` : '/post/upload'
}

/** Recursively collect every string value nested in `value`. */
function collectStrings(value: unknown, out: string[] = []): string[] {
  if (typeof value === 'string') {
    out.push(value)
  } else if (Array.isArray(value)) {
    for (const item of value) collectStrings(item, out)
  } else if (value !== null && typeof value === 'object') {
    for (const v of Object.values(value as Record<string, unknown>)) collectStrings(v, out)
  }
  return out
}

/** Basename of a (possibly path-like) stored name. */
function baseName(name: string): string {
  const parts = name.split(/[/\\]/)
  return parts[parts.length - 1] ?? name
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
      autoRename: true,
      // Number of files in the batch being uploaded.
      fileCount: 0,
      // Target directory reported by the server for the batch; every stored
      // file landed directly in it, so it scopes the wait-for-indexed poll.
      uploadedDir: undefined as string | undefined,
      // Per-file progress tracking for the sequential queue.
      currentFileName: undefined as string | undefined,
      currentFileSize: undefined as number | undefined,
      currentIndex: 0,
      completedBytes: 0,
      totalBytes: 0,
      uploadedNames: [] as string[]
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

        const uploadUrl = buildUploadUrl(albumId, autoRename)

        this.fileCount = files.length
        this.uploadedNames = []
        this.uploadedDir = undefined
        this.completedBytes = 0
        this.totalBytes = files.reduce((sum, f) => sum + f.size, 0)
        this.loaded = 0
        this.total = this.totalBytes
        this.startTime = Date.now()
        this.currentIndex = 0
        this.currentFileName = undefined
        this.currentFileSize = undefined

        const abortController = new AbortController()
        this.abortController = abortController

        try {
          for (let i = 0; i < files.length; i++) {
            const file = files[i]
            if (file === undefined) continue
            this.currentIndex = i + 1
            this.currentFileName = file.name
            this.currentFileSize = file.size

            const formData = new FormData()
            formData.append('file', file)
            formData.append('lastModified', String(file.lastModified))

            const requestController = new AbortController()
            this.abortController = requestController

            const response = await axios.post(uploadUrl, formData, {
              headers: { 'Content-Type': 'multipart/form-data' },
              signal: requestController.signal,
              onUploadProgress: (e: AxiosProgressEvent) => {
                if (typeof e.loaded === 'number') {
                  this.loaded = this.completedBytes + e.loaded
                }
                this.total = this.totalBytes
              }
            })

            this.completedBytes += file.size
            this.loaded = this.completedBytes

            const serverFiles = (response.data as { files?: unknown } | undefined)?.files
            if (Array.isArray(serverFiles)) {
              for (const name of serverFiles) {
                if (typeof name === 'string') this.uploadedNames.push(name)
              }
            }
            // Target directory reported by the server; sanitize/rename may
            // have changed file names, but everything landed in this dir.
            const serverDir = (response.data as { dir?: unknown } | undefined)?.dir
            if (typeof serverDir === 'string' && serverDir !== '') {
              this.uploadedDir = serverDir
            }
          }

          // Files are stored; wait for them to be indexed before refreshing
          // so the grid shows them. Distinct status so the UI does not keep
          // claiming "Uploading" with no bytes left to send.
          this.status = 'Indexing'
          await this.waitForIndexed(this.uploadedDir, albumId, this.uploadedNames)

          this.status = 'Completed'
          messageStore.success('Files uploaded successfully')
          // Same grid reload as assign-album/batch-tag mutations; a failed
          // refresh must not flip the finished upload back to Canceled.
          try {
            // The router module pulls in the whole page tree, so load it only
            // when a refresh is actually needed.
            const { default: router } = await import('@/route/routes')
            await refreshGalleryAfterMutation(isolationId, router.currentRoute.value)
          } catch {
            // No-op: the upload itself succeeded and is already toasted.
          }
        } catch (err: unknown) {
          this.status = 'Canceled'
          // The global axios interceptor already displayed axios errors
          // that carry a response; surface only what it could not
          // (non-axios throws and responseless failures).
          if (!axios.isAxiosError(err) || err.response === undefined) {
            messageStore.error(errorDisplay(err))
          }
        }
      },

      async waitForIndexed(
        uploadedDir: string | undefined,
        albumId: string | undefined,
        uploadedNames: string[]
      ): Promise<void> {
        if (uploadedNames.length === 0) return
        const targets = uploadedNames.map(baseName)
        // Prefer the server-reported directory: every stored file landed
        // directly in it, so dir + basename matching is exact (filesystem
        // uniqueness). Fall back to the album scope, then to an unfiltered
        // poll, when the server did not report a dir.
        const filter =
          uploadedDir !== undefined
            ? `and(trashed:false, Path:"${uploadedDir}")`
            : albumId !== undefined
              ? `and(trashed:false, or(album:"${albumId}", parent_album:"${albumId}"))`
              : null

        const deadline = Date.now() + 60000
        while (Date.now() < deadline) {
          try {
            const result = await prefetch(filter, undefined, undefined, null, false)
            const strings = collectStrings(result)
            const found = targets.every((t) => strings.some((s) => baseName(s) === t))
            if (found) return
          } catch {
            // ignore and retry
          }
          await new Promise<void>((resolve) => setTimeout(resolve, 1000))
        }
        // Timeout: refresh anyway (rows arrive via backstop later).
      },

      cancelUpload(): void {
        if (this.abortController) {
          this.abortController.abort()
        }
        this.pendingFiles = []
        this.pendingAlbumId = undefined
        this.status = 'Canceled'
      }
    }
  })()
