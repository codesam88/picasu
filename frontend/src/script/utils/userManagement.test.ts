import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/// UserManagement is a .vue file with no mount harness under node vitest
/// (same reason routes.test.ts checks source instead of importing it), so
/// the removal of its duplicate 401 navigation is pinned structurally.
const source = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/Config/UserManagement.vue'),
  'utf-8'
)

describe('UserManagement admin shield toggle', () => {
  test('shield button feeds a real boolean to onToggle, not an event', () => {
    // A v-btn click carries no value payload, so the call site passes the
    // flipped flag explicitly. No v-switch remains: no ChangeEvent vs
    // update:modelValue ambiguity is possible at all.
    expect(source).toMatch(/@click="onToggle\(user, !user\.admin\)"/)
    expect(source).not.toMatch(/v-switch/)
    expect(source).not.toMatch(/@update:model-value="onToggle/)
    expect(source).not.toMatch(/@change="onToggle/)
  })

  test('shield icon and color reflect admin state at icon-button size', () => {
    expect(source).toMatch(/mdi-shield-crown/)
    expect(source).toMatch(/mdi-shield-outline/)
    expect(source).toMatch(/aria-pressed/)
  })

  test('shield sits between the key and delete actions', () => {
    const key = source.indexOf('mdi-key-variant')
    const shield = source.indexOf('mdi-shield-crown')
    const trash = source.indexOf('mdi-delete-outline')
    expect(key).toBeGreaterThanOrEqual(0)
    expect(shield).toBeGreaterThanOrEqual(0)
    expect(trash).toBeGreaterThanOrEqual(0)
    expect(key).toBeLessThan(shield)
    expect(shield).toBeLessThan(trash)
  })

  test('hover and accessible names describe the action', () => {
    expect(source).toMatch(/Demote admin/)
    expect(source).toMatch(/Make admin/)
  })
})

describe('UserManagement row-action labels', () => {
  test('icon buttons expose hover labels and accessible names', () => {
    // Bare icon buttons carry no affordance; the house pattern (LinksPage,
    // GalleryBar) is v-tooltip + aria-label.
    expect(source).toMatch(/text="Set password"/)
    expect(source).toMatch(/text="Delete user"/)
    expect(source).toMatch(/aria-label="Set password"/)
    expect(source).toMatch(/aria-label="Delete user"/)
  })
})

describe('UserManagement grouped display', () => {
  test('renders role groups with a divider only between lists', () => {
    expect(source).toMatch(/groupUsersByRole\(/)
    expect(source).toMatch(/v-for="\(group, gi\) in userGroups"/)
    expect(source).toMatch(/<v-divider v-if="gi > 0"/)
  })

  test('role is conveyed by grouping, not a per-row subtitle', () => {
    expect(source).not.toMatch(/Standard user/)
    expect(source).toMatch(/Promote someone else first/)
  })
})

describe('UserManagement single-owner display', () => {
  test('no duplicate navigation to login (interceptor owns the redirect)', () => {
    expect(source).not.toMatch(/router\.push/)
    expect(source).not.toMatch(/redirectLogin/)
  })

  test('gap-fill guard still toasts only what the interceptor could not display', () => {
    expect(source).toMatch(/axios\.isAxiosError\(error\)/)
    expect(source).toMatch(/messageStore\.error\(errorDisplay\(error\)\)/)
  })
})
