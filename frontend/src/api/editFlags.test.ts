import { describe, expect, test } from 'vitest'
import * as editFlagsApi from '@/api/editFlags'

/**
 * `edit_flags` now carries the trash flag only: favorite and archived were
 * removed along with the fields they toggled, so their convenience helpers
 * must not come back.
 */
describe('editFlags exports', () => {
  test('only the trash helper survives', () => {
    expect(Object.keys(editFlagsApi).sort()).toEqual(['editFlags', 'setTrashed'])
  })
})
