import axios from 'axios'
import { z } from 'zod'

export interface AppConfig {
  limits: Record<string, string>

  // App settings
  readOnlyMode: boolean
  disableImg: boolean
  fsNotifyWatcher: boolean
  // password is handled separately now
  authKey: string | null
  hasAuthKey: boolean
  imagePath: string | null
  uploadFolder: string | null
  maxUploadSize: string
}

export const getConfig = async (): Promise<AppConfig> => {
  const response = await axios.get<AppConfig>('/get/config')
  return response.data
}

export const updateConfig = async (config: Partial<AppConfig>): Promise<void> => {
  await axios.put('/put/config', config)
}

/// Rotate the JWT signing key. The server generates a new strong random key
/// and installs it; all sessions invalidate. The key itself is never
/// returned — only whether one is now configured.
export const resetAuthKey = async (): Promise<{ hasAuthKey: boolean }> => {
  const response = await axios.post('/post/config/auth-key/reset')
  return z.object({ hasAuthKey: z.boolean() }).parse(response.data)
}

export const exportConfig = async (): Promise<AppConfig> => {
  const response = await axios.get<AppConfig>('/get/config/export')
  return response.data
}

export const importConfig = async (config: AppConfig): Promise<void> => {
  // Refactor: path to /post/config/import
  await axios.post('/post/config/import', config)
}
