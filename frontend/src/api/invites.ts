import axios from 'axios'
import { z } from 'zod'

export const InviteViewSchema = z.object({
  id: z.string(),
  prefix: z.string(),
  createdBy: z.string(),
  expiresAt: z.number().nullable(),
  usedBy: z.string().nullable(),
  username: z.string().nullable()
})

export type InviteView = z.infer<typeof InviteViewSchema>

export const createInviteResponseSchema = z.object({
  token: z.string(),
  invite: InviteViewSchema
})

export type CreateInviteResponse = z.infer<typeof createInviteResponseSchema>

export const createInvite = async (
  expiresInHours: number,
  username?: string
): Promise<CreateInviteResponse> => {
  const body: { expiresInHours: number; username?: string } = { expiresInHours }
  // Omit the username when empty so the invite stays open.
  if (username !== undefined && username !== '') {
    body.username = username
  }
  const response = await axios.post('/post/invites', body)
  return createInviteResponseSchema.parse(response.data)
}

export const listInvites = async (): Promise<InviteView[]> => {
  const response = await axios.get('/get/invites')
  return z.array(InviteViewSchema).parse(response.data)
}

export const revokeInvite = async (inviteId: string): Promise<void> => {
  await axios.delete(`/delete/invites/${encodeURIComponent(inviteId)}`)
}

/// One-time invite link: the bound username travels as a display hint
/// (`?username=`); the token stays the bearer. Dropping or editing the
/// hint only changes the prefill — redeem still enforces the server-side
/// binding (403 on mismatch).
export const buildInviteLink = (origin: string, token: string, username?: string): string => {
  const trimmed = username?.trim() ?? ''
  const base = `${origin}/register?invite=${encodeURIComponent(token)}`
  return trimmed !== '' ? `${base}&username=${encodeURIComponent(trimmed)}` : base
}
