import type { UserSummary } from '@/api/users'

export interface UserGroup {
  key: 'admin' | 'standard'
  users: UserSummary[]
}

/// Display order for the admin console: admins first, then standard users,
/// each sorted by user id. Empty groups are omitted so the template can
/// place a divider between rendered groups without guarding edge cases;
/// an empty store yields no groups at all.
export const groupUsersByRole = (users: UserSummary[]): UserGroup[] => {
  const byId = (a: UserSummary, b: UserSummary) => a.userId.localeCompare(b.userId)
  const admins = users.filter((u) => u.admin).sort(byId)
  const standards = users.filter((u) => !u.admin).sort(byId)
  const groups: UserGroup[] = []
  if (admins.length > 0) groups.push({ key: 'admin', users: admins })
  if (standards.length > 0) groups.push({ key: 'standard', users: standards })
  return groups
}
