import axios from 'axios'
import { z } from 'zod'

export const selfProfileUserSchema = z.object({
  userId: z.string(),
  admin: z.boolean(),
  home: z.string().nullable()
})

// eslint-disable-next-line @typescript-eslint/no-deprecated -- zod v4 keeps passthrough(); migrating the settings schema to z.looseObject() is a separate zod-API change
export const selfProfileSettingsSchema = z.object({}).passthrough()

export const selfProfileSchema = z.object({
  user: selfProfileUserSchema,
  settings: selfProfileSettingsSchema
})

export type SelfProfileUser = z.infer<typeof selfProfileUserSchema>
export type SelfProfileSettings = z.infer<typeof selfProfileSettingsSchema>
export type SelfProfile = z.infer<typeof selfProfileSchema>

export const getSelfProfile = async (): Promise<SelfProfile> => {
  const response = await axios.get('/get/self/profile')
  return selfProfileSchema.parse(response.data)
}
