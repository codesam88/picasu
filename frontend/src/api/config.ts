import axios from 'axios'
import { z } from 'zod'

export interface AppConfig {
  limits: Record<string, string>

  // App settings
  readOnlyMode: boolean
  fsNotifyWatcher: boolean
  normalizeUploadFilenames?: boolean
  validateUploadContent?: boolean
  useClientTimestampInfo?: boolean
  allowRegistration?: boolean
  personalAlbumsEnabled?: boolean
  // password is handled separately now
  authKey: string | null
  uploadFolder: string | null
  maxUploadSize: string
}

export const getConfig = async (): Promise<AppConfig> => {
  const response = await axios.get<AppConfig>('/get/config')
  return response.data
}

/// Rotate the JWT signing key. The server generates a new strong random key
/// and installs it; all sessions invalidate. The key itself is never
/// returned — only whether one is now configured.
export const resetAuthKey = async (): Promise<{ hasAuthKey: boolean }> => {
  const response = await axios.post('/post/config/auth-key/reset')
  return z.object({ hasAuthKey: z.boolean() }).parse(response.data)
}
