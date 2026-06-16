import axios from 'axios'

export interface FsCompletion {
  roots: string[]
  children: string[]
  is_default: boolean
}

export const fetchFsCompletion = async (path: string): Promise<FsCompletion> => {
  const response = await axios.get<FsCompletion>('/get/path-completion', {
    params: { path }
  })
  return response.data
}

export type FolderImportState = 'idle' | 'running' | 'completed' | 'canceled' | 'failed'

export interface FolderImportStatus {
  state: FolderImportState
  root: string | null
  scanned: number
  matched: number
  processed: number
  failed: number
  startedAt: number | null
  finishedAt: number | null
  cancelRequested: boolean
}

export const startFolderImport = async (path: string): Promise<void> => {
  await axios.post('/post/import/folder', { path })
}

/**
 * Scan the configured imagePath for files the watcher hasn't indexed yet
 * (e.g. pre-existing files dropped in before the app last started). Unlike
 * startFolderImport, takes no path — always targets the configured root, so
 * albums/hierarchy are reliably discovered. Shares the same job slot/status
 * as a regular folder import (see getFolderImportStatus).
 *
 * `force` (default false): also re-run full metadata extraction for files
 * whose content hash is already indexed, not just newly-discovered ones —
 * for fixing inconsistencies or properly indexing a pre-existing file repo.
 */
export const startImageHomeScan = async (force = false): Promise<void> => {
  await axios.post('/post/import/image-home', null, { params: { force } })
}

export const getFolderImportStatus = async (): Promise<FolderImportStatus> => {
  const response = await axios.get<FolderImportStatus>('/get/import/folder/status')
  return response.data
}

export const cancelFolderImport = async (): Promise<void> => {
  await axios.post('/post/import/folder/cancel')
}
