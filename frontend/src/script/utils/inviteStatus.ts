import type { InviteView } from '@/api/invites'

export type InviteStatus = 'unused' | 'used' | 'expired' | 'revoked'

/// Derive the display status of an invite from the backend's fields.
/// `usedBy` marks it used; an `expiresAt` in the past marks it expired;
/// a locally tracked revoke marks it revoked; otherwise unused.
export function inviteStatus(
  invite: InviteView,
  nowSeconds: number = Date.now() / 1000,
  revoked: boolean = false
): InviteStatus {
  if (revoked) return 'revoked'
  if (invite.usedBy !== null) return 'used'
  if (invite.expiresAt !== null && nowSeconds >= invite.expiresAt) return 'expired'
  return 'unused'
}
