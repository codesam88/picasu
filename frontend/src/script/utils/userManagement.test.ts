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

describe('UserManagement admin toggle wiring', () => {
  test('routes the switch boolean via update:model-value, not native change', () => {
    // VSwitch emits only update:modelValue (see VSwitch.js emits). A native
    // @change handler receives a ChangeEvent, so `value === true` is never
    // true and every promotion mis-routes into the demote-confirm branch.
    expect(source).toMatch(/@update:model-value="onToggle\(user, \$event\)"/)
    expect(source).not.toMatch(/@change="onToggle/)
  })

  test('uses controlled model-value so the confirm flow owns the mutation', () => {
    // v-model auto-mutates before onToggle runs, forcing a revert-and-race
    // with the demote confirm dialog. Every other admin switch in the app
    // (ConfigPage, AdvancedConfig, FrontendConfig) uses controlled
    // :model-value + @update:model-value instead.
    expect(source).toMatch(/:model-value="user\.admin"/)
    expect(source).not.toMatch(/v-model="user\.admin"/)
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
