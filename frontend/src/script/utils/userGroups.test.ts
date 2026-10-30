import { describe, expect, test } from 'vitest'
import { groupUsersByRole } from './userGroups'

describe('groupUsersByRole', () => {
  test('admins come first, each group sorted by user id', () => {
    const groups = groupUsersByRole([
      { userId: 'zed', admin: false },
      { userId: 'bob', admin: true },
      { userId: 'amy', admin: false },
      { userId: 'ann', admin: true }
    ])
    expect(groups.map((g) => g.key)).toEqual(['admin', 'standard'])
    const [adminGroup, standardGroup] = groups
    expect(adminGroup?.users.map((u) => u.userId)).toEqual(['ann', 'bob'])
    expect(standardGroup?.users.map((u) => u.userId)).toEqual(['amy', 'zed'])
  })

  test('omits empty groups so a divider only ever sits between two lists', () => {
    expect(groupUsersByRole([{ userId: 'root', admin: true }]).map((g) => g.key)).toEqual(['admin'])
    expect(groupUsersByRole([{ userId: 'bob', admin: false }]).map((g) => g.key)).toEqual([
      'standard'
    ])
  })

  test('empty store yields no groups', () => {
    expect(groupUsersByRole([])).toEqual([])
  })

  test('does not mutate the input order', () => {
    const input = [
      { userId: 'zed', admin: false },
      { userId: 'ann', admin: true }
    ]
    groupUsersByRole(input)
    expect(input.map((u) => u.userId)).toEqual(['zed', 'ann'])
  })
})
