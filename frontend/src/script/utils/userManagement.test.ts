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
    // GalleryBar) is v-tooltip + aria-label. Set-password keeps static
    // strings; shield and delete bind dynamic labels (own tests below).
    expect(source).toMatch(/text="Set password"/)
    expect(source).toMatch(/aria-label="Set password"/)
  })
})

describe('UserManagement list framing', () => {
  test('rows sit in an inset sub-table, never full-bleed', () => {
    // The create form lives in v-card-text; bare rows as direct card
    // children bleed edge to edge and read as outside the frame.
    expect(source).toMatch(/<v-sheet[\s\S]*?border/)
    expect(source).toMatch(/v-if="userGroups\.length > 0"/)
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
    expect(source).not.toMatch(/#subtitle/)
    expect(source).toMatch(/promote another user first/)
  })

  test('lock hint lives on the shield tooltip, readable while disabled', () => {
    // A disabled button fires no hover events, so the tooltip activator is
    // a wrapper span; the label picks the locked phrasing when demote is
    // locked, since no subtitle explains the row anymore.
    expect(source).toMatch(/<span v-bind="props"/)
    expect(source).toMatch(/:text="shieldLabel\(user\)"/)
    expect(source).toMatch(/:aria-label="shieldLabel\(user\)"/)
    expect(source).toMatch(/Cannot demote admin - promote another user first/)
  })

  test('locked delete stays visible as disabled with a reason', () => {
    // Hiding the action teaches nothing; like the shield, the trash stays
    // rendered but disabled, with the tooltip carrying the reason. Both
    // tooltips need the span activator since disabled buttons fire no hover.
    expect(source).not.toMatch(/v-if="!isRowLocked\(user\)"/)
    expect(source).toMatch(/:text="deleteLabel\(user\)"/)
    expect(source).toMatch(/:aria-label="deleteLabel\(user\)"/)
    expect(source).toMatch(/Cannot delete admin - promote another user first/)
    expect(source.match(/<span v-bind="props"/g)).toHaveLength(2)
  })
})

describe('UserManagement promote confirmation', () => {
  test('promotion confirms like demotion, with its own prompt', () => {
    expect(source).toMatch(/Make admin\?/)
    expect(source).toMatch(/Demote admin\?/)
  })

  test('shield toggle stages a confirm instead of applying the role itself', () => {
    // The toggle must never flip admin state or call the API inline —
    // granting admin is too destructive for an unconfirmed click. Scope
    // to the toggle body so unrelated confirms cannot satisfy the pin.
    const toggleStart = source.indexOf('const onToggle')
    const toggleEnd = source.indexOf('const cancelRoleChange')
    expect(toggleStart).toBeGreaterThanOrEqual(0)
    expect(toggleEnd).toBeGreaterThan(toggleStart)
    const toggleBody = source.slice(toggleStart, toggleEnd)
    expect(toggleBody).toMatch(/pendingTarget\.value = user/)
    expect(toggleBody).toMatch(/confirmOpen\.value = true/)
    expect(toggleBody).not.toMatch(/applyRoleChange|setUserAdmin/)
    expect(toggleBody).not.toMatch(/\.admin =/)
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
