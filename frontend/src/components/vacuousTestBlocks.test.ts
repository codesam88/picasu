import { readFileSync, readdirSync } from 'node:fs'
import { join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, test } from 'vitest'

/// Meta-scan in the designStructure style: a test file that pins
/// component or route text via readFileSync must keep at least one
/// positive assertion per test block. An absence-only block passes even
/// when the feature it guards is deleted, so new ones fail here until
/// they are paired with a positive assertion or ticketed for a behavior
/// rewrite. Behavioral negatives (mock never called after driving real
/// code) live in files that import the code instead of reading it and
/// are out of scope by construction.

const srcDir = join(fileURLToPath(new URL('.', import.meta.url)), '..')

interface Finding {
  file: string
  block: string
}

interface GrandfatheredBlock extends Finding {
  reason: string
}

/// Absence-only blocks grandfathered by file + block name. Each entry
/// shrinks by converting the block (pair the negation with a positive
/// assertion) or ticketing a rewrite into a behavior test.
const KNOWN_VACUOUS: readonly GrandfatheredBlock[] = [
  {
    file: 'script/utils/shareLoginModal.test.ts',
    block: 'share 403 pushes no local snackbar (interceptor owns share-403 display)',
    reason: 'convert-via-pair: pin the interceptor-owned share-403 display beside the modal silence'
  },
  {
    file: 'script/utils/changePasswordForm.test.ts',
    block: 'no longer calls the deprecated config/password alias',
    reason: 'convert-via-pair: pin the canonical setUserPassword route in the same block'
  },
  {
    file: 'script/utils/createShareModal.test.ts',
    block: 'create path has no local error snackbar (interceptor owns axios display)',
    reason: 'convert-via-pair: pin the tryWithMessageStore wiring in the same block'
  },
  {
    file: 'script/utils/userManagement.test.ts',
    block: 'no duplicate navigation to login (interceptor owns the redirect)',
    reason: 'convert-via-pair: pin the interceptor-owned redirect beside the silence check'
  },
  {
    file: 'route/registerRoute.test.ts',
    block: 'S7 guard performs no probe and holds no cache',
    reason: 'rewrite-ticket: R31 source tripwire superseded by bootGuard.test.ts behavior tests'
  },
  {
    file: 'script/utils/feat6AdminPanel.test.ts',
    block: 'link is not persisted to storage (shown once)',
    reason: 'convert-via-pair: pin the once-only invite-link alert rendering positively'
  },
  {
    file: 'script/utils/shareAlbumMenu.test.ts',
    block: 'GalleryBar keeps no share button and mounts no share dialog',
    reason: 'convert-via-pair: pin the GalleryBar toolbar wiring beside the absence check'
  }
]

const TEST_CALL = /\b(?:test|it)\s*\(\s*(['"`])((?:\\.|(?!\1).)*)\1/g
const EXPECT_SPLIT = /(?=\bexpect(?:\.(?:soft|poll))?\s*\()/
const POSITIVE_MATCHER =
  /\.(?:toEqual\w*|toContain\w*|toMatch\w*|toHaveLength|toBeGreaterThan\w*|toBeLessThan\w*|toBeInstanceOf|toBeDefined|toBeTruthy|toHaveProperty|toHaveBeenCalled\w*|toBeCalled\w*|toThrow\w*|toBeVisible|toBeHidden|toBeCloseTo|toBe)\(/

function walk(dir: string): string[] {
  const found: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) found.push(...walk(path))
    else if (entry.isFile()) found.push(path)
  }
  return found
}

/// Test files that read source text (components, routes, worker modules)
/// instead of importing it — the only files where absence-only blocks
/// slip through without any stimulus.
function sourceReadingFiles(): { file: string; source: string }[] {
  return walk(srcDir)
    .filter((path) => /\.(test|spec)\.ts$/.test(path))
    .map((path) => ({ file: path, source: readFileSync(path, 'utf8') }))
    .filter((entry) => entry.source.includes('readFileSync('))
}

/// Split a file into test/it blocks: a body spans from the end of one
/// call's header to the start of the next (test bodies never nest).
function testBlocks(source: string): { name: string; body: string }[] {
  const marks: { name: string; end: number; start: number }[] = []
  TEST_CALL.lastIndex = 0
  let match = TEST_CALL.exec(source)
  while (match !== null) {
    const name = match[2]
    if (name !== undefined) {
      marks.push({ name, start: match.index, end: match.index + match[0].length })
    }
    match = TEST_CALL.exec(source)
  }
  return marks.map((mark, index) => ({
    name: mark.name,
    body: source.slice(mark.end, marks[index + 1]?.start ?? source.length)
  }))
}

/// A window (one expect call up to the next) is positive when it carries
/// an assertion matcher that is not negated by `.not.`. Absence matchers
/// (toBeNull, toBeFalsy, toBeUndefined), negated matchers, and
/// unrecognized matchers are not positive; a block with no positive
/// window is vacuous.
function isPositive(window: string): boolean {
  const positiveAt = window.search(POSITIVE_MATCHER)
  if (positiveAt === -1) return false
  const negatedAt = window.indexOf('.not.')
  return negatedAt === -1 || positiveAt < negatedAt
}

function hasPositiveAssertion(body: string): boolean {
  // The first split piece is the setup before the first expect; every
  // later piece runs from one expect call to the next.
  return body.split(EXPECT_SPLIT).slice(1).some(isPositive)
}

function vacuousBlocks(): Finding[] {
  const vacuous: Finding[] = []
  for (const { file, source } of sourceReadingFiles()) {
    const rel = relative(srcDir, file).split(sep).join('/')
    for (const block of testBlocks(source)) {
      if (!hasPositiveAssertion(block.body)) vacuous.push({ file: rel, block: block.name })
    }
  }
  return vacuous
}

function keyOf(finding: Finding): string {
  return `${finding.file}::${finding.block}`
}

describe('source-reading test files', () => {
  test('contain no absence-only blocks beyond the grandfathered list', () => {
    const grandfathered = new Set(KNOWN_VACUOUS.map(keyOf))
    const detail = vacuousBlocks()
      .filter((finding) => !grandfathered.has(keyOf(finding)))
      .map((finding) => `${finding.file} :: ${finding.block}`)
    expect(
      detail,
      `vacuous absence-only test blocks (pair with a positive assertion or ticket a rewrite):\n${detail.join('\n')}`
    ).toEqual([])
  })

  test('grandfathered absence-only blocks are still present and vacuous', () => {
    const found = new Set(vacuousBlocks().map(keyOf))
    const detail = KNOWN_VACUOUS.filter((entry) => !found.has(keyOf(entry))).map(
      (entry) => `${entry.file} :: ${entry.block}`
    )
    expect(
      detail,
      `stale KNOWN_VACUOUS entries (block converted, renamed, or removed):\n${detail.join('\n')}`
    ).toEqual([])
  })
})
