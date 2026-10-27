import axios from 'axios'
import { z } from 'zod'

export const configFieldEntrySchema = z.object({
  name: z.string(),
  value: z.unknown(),
  tier: z.string(),
  source: z.string(),
  requiresRestart: z.boolean(),
  writable: z.boolean()
})

export type ConfigFieldEntry = z.infer<typeof configFieldEntrySchema>

export const runtimeConfigSchema = z.object({
  uploadFolder: z.string(),
  maxUploadSize: z.string(),
  readOnlyMode: z.boolean(),
  fsNotifyWatcher: z.boolean(),
  normalizeUploadFilenames: z.boolean(),
  validateUploadContent: z.boolean(),
  useClientTimestampInfo: z.boolean(),
  allowRegistration: z.boolean()
})

export type RuntimeConfigDocument = z.infer<typeof runtimeConfigSchema>

export const adminConfigUpdateResponseSchema = z.object({
  config: runtimeConfigSchema,
  restartRequired: z.array(z.string())
})

export type AdminConfigUpdateResponse = z.infer<typeof adminConfigUpdateResponseSchema>

export const adminConfigExportResponseSchema = z.object({
  version: z.number(),
  scope: z.string(),
  config: runtimeConfigSchema
})

export type AdminConfigExportResponse = z.infer<typeof adminConfigExportResponseSchema>

export type RuntimeConfigPatch = Partial<RuntimeConfigDocument>

export const getAdminConfig = async (): Promise<ConfigFieldEntry[]> => {
  const response = await axios.get('/get/admin/config')
  return z.array(configFieldEntrySchema).parse(response.data)
}

export const updateAdminConfig = async (
  config: RuntimeConfigPatch
): Promise<AdminConfigUpdateResponse> => {
  const response = await axios.put('/put/admin/config', config)
  return adminConfigUpdateResponseSchema.parse(response.data)
}

export const disableReadOnly = async (password: string): Promise<void> => {
  await axios.post('/post/admin/config/disable-read-only', null, {
    headers: { 'X-Picasu-Password': password }
  })
}

export const exportAdminConfig = async (): Promise<AdminConfigExportResponse> => {
  const response = await axios.get('/get/admin/config/export')
  return adminConfigExportResponseSchema.parse(response.data)
}
