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

export type AlbumIndexState = 'idle' | 'running' | 'completed' | 'canceled' | 'failed'

export interface AlbumIndexStatus {
  state: AlbumIndexState
  root: string | null
  scanned: number
  matched: number
  processed: number
  failed: number
  startedAt: number | null
  finishedAt: number | null
  cancelRequested: boolean
}

export const startAlbumIndex = async (album?: string): Promise<void> => {
  await axios.post('/post/index/album', { album: album ?? '/' })
}

export const getAlbumIndexStatus = async (): Promise<AlbumIndexStatus> => {
  const response = await axios.get<AlbumIndexStatus>('/get/index/status')
  return response.data
}

export const cancelAlbumIndex = async (): Promise<void> => {
  await axios.post('/post/index/cancel')
}
