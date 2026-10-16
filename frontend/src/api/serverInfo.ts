import axios from 'axios'
import { z } from 'zod'

const serverInfoSchema = z.object({
  enrolled: z.boolean(),
  enrollmentOpen: z.boolean(),
  captcha: z.null()
})

export type ServerInfo = z.infer<typeof serverInfoSchema>

/// Anonymous boot signal: whether any user is enrolled and whether
/// first-admin enrollment is currently open. `captcha` is always null
/// (reserved for R7). Hand-parsed with zod so malformed shapes reject.
export const getServerInfo = async (): Promise<ServerInfo> => {
  const response = await axios.get('/get/server-info')
  return serverInfoSchema.parse(response.data)
}
