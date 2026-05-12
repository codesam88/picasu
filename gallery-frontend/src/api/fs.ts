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

export const getFolderImportStatus = async (): Promise<FolderImportStatus> => {
  const response = await axios.get<FolderImportStatus>('/get/import/folder/status')
  return response.data
}

export const cancelFolderImport = async (): Promise<void> => {
  await axios.post('/post/import/folder/cancel')
}
