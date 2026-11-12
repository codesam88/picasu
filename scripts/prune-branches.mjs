#!/usr/bin/env node
// Prune stale local and remote branches.
//
// Dry run by default. The default cutoffs are always active; --apply deletes.
//   --expire-local DAYS   override the local merged/no-exact-PR cutoff
//   --expire-closed DAYS  override the local/remote PR merged/closed cutoff
//   --expire-gone DAYS    override the upstream-gone cutoff
//   --expire-archived DAYS
//                         override the refs/archive/<date>/... cutoff
// Never touched: the base and default branches, the current branch, branches
// checked out in any worktree, branches with an open pull request, and
// --protect globs. A local branch deleted without being merged into the base
// is archived under refs/archive/<date>/<branch> unless --no-archive.
//
// Usage: prune-branches.mjs [--expire-local DAYS] [--expire-closed DAYS]
//                           [--expire-gone DAYS] [--expire-archived DAYS]
//                           [--apply] [--base BRANCH] [--remote NAME]
//                           [--protect GLOB] [--no-archive] [--no-fetch]
//                           [--help]
//
// Reporting flow: collect -> classify -> sort -> execute -> render. A Decision
// carries its category, action, evidence and result; rendering only groups and
// formats decisions and never re-runs classification or counting.
import { spawnSync } from 'node:child_process'

const DEFAULT_DAYS = 14
const DEFAULT_GONE_DAYS = 7
const DEFAULT_ARCHIVED_DAYS = 30
const PR_LIMIT = 2000
const gh = 'gh'

/**
 * Category registry. Order is the render order. `selector` is the only option
 * that can turn this category into deletions; `sort` is `name`, `pr` or `tip`.
 */
const CATEGORIES = [
  { id: 'protected', title: 'Protected', selector: null, sort: 'name' },
  {
    id: 'local.merged-base',
    title: 'Local merged into base',
    selector: '--expire-local',
    sort: 'name',
  },
  {
    id: 'local.pr-merged',
    title: 'Local PR merged',
    selector: '--expire-closed',
    sort: 'pr',
  },
  {
    id: 'local.pr-closed',
    title: 'Local PR closed',
    selector: '--expire-closed',
    sort: 'pr',
  },
  { id: 'local.gone', title: 'Upstream gone', selector: '--expire-gone', sort: 'tip' },
  { id: 'local.no-exact-pr', title: 'Local no exact PR', selector: '--expire-local', sort: 'tip' },
  {
    id: 'remote.pr-merged',
    title: 'Remote PR merged',
    selector: '--expire-closed',
    sort: 'pr',
  },
  {
    id: 'remote.pr-closed',
    title: 'Remote PR closed',
    selector: '--expire-closed',
    sort: 'pr',
  },
  {
    id: 'remote.no-pr',
    title: 'Remote no PR',
    selector: null,
    sort: 'name',
  },
  { id: 'archived', title: 'Archived', selector: '--expire-archived', sort: 'archive' },
]

const CATEGORY_BY_ID = new Map(CATEGORIES.map((category) => [category.id, category]))
const CATEGORY_ORDER = new Map(CATEGORIES.map((category, index) => [category.id, index]))

const usage = `Usage: prune-branches.mjs [options]

Selects stale branches and reports them. Deletion requires --apply; the
expire options override the defaults.

Options:
  --expire-local DAYS   Override the local merged/no-exact-PR cutoff
                        (default ${DEFAULT_DAYS}).
  --expire-closed DAYS  Override the local/remote PR merged/closed cutoff
                        (default ${DEFAULT_DAYS}).
  --expire-gone DAYS    Override the upstream-gone cutoff
                        (default ${DEFAULT_GONE_DAYS}).
  --expire-archived DAYS
                        Override the archived-ref cutoff
                        (default ${DEFAULT_ARCHIVED_DAYS}).
  --apply               Delete the selected branches/refs; otherwise only report.
  --base BRANCH         Branch to test merges against (default main).
  --remote NAME         Git remote to prune (default origin).
  --protect GLOB        Additional branch pattern to keep (repeatable;
                        defaults: backup/*, release/*).
  --no-archive          Do not record refs/archive/<date>/<branch> before
                        deleting a local branch that is not merged into --base.
  --no-fetch            Skip 'git fetch --prune' before and after the analysis.
  -h, --help            Show this help.

Defaults: local/closed ${DEFAULT_DAYS} days, gone ${DEFAULT_GONE_DAYS} days, archived
${DEFAULT_ARCHIVED_DAYS} days.
`

const options = {
  apply: false,
  expireLocalDays: undefined,
  expireClosedDays: undefined,
  expireGoneDays: undefined,
  expireArchivedDays: undefined,
  base: 'main',
  remote: 'origin',
  archive: true,
  fetch: true,
  protect: ['backup/*', 'release/*'],
}

for (let index = 2; index < process.argv.length; index += 1) {
  const arg = process.argv[index]
  switch (arg) {
    case '--apply':
      options.apply = true
      break
    case '--expire-local':
      options.expireLocalDays = parseDays(requireValue(index, arg), arg)
      index += 1
      break
    case '--expire-closed':
      options.expireClosedDays = parseDays(requireValue(index, arg), arg)
      index += 1
      break
    case '--expire-gone':
      options.expireGoneDays = parseDays(requireValue(index, arg), arg)
      index += 1
      break
    case '--expire-archived':
      options.expireArchivedDays = parseDays(requireValue(index, arg), arg)
      index += 1
      break
    case '--days':
      fail(
        '--days was replaced by --expire-local DAYS, --expire-closed DAYS, --expire-gone DAYS and --expire-archived DAYS',
      )
      break
    case '--base':
      options.base = requireValue(index, arg)
      index += 1
      break
    case '--remote':
      options.remote = requireValue(index, arg)
      index += 1
      break
    case '--protect':
      options.protect.push(requireValue(index, arg))
      index += 1
      break
    case '--force-gone':
      fail('--force-gone was replaced by --expire-gone DAYS')
      break
    case '--no-archive':
      options.archive = false
      break
    case '--no-fetch':
      options.fetch = false
      break
    case '-h':
    case '--help':
      process.stdout.write(usage)
      process.exit(0)
      break
    default:
      fail(`unknown option: ${arg}`)
  }
}

function parseDays(value, flag) {
  if (!/^\d+$/.test(value)) {
    fail(`${flag} needs a non-negative integer, got ${value}`)
  }
  return Number(value)
}

function requireValue(index, flag) {
  const value = process.argv[index + 1]
  if (value === undefined || value.startsWith('--')) {
    fail(`${flag} needs a value`)
  }
  return value
}

function fail(message) {
  process.stderr.write(`prune-branches: ${message}\n\n${usage}`)
  process.exit(2)
}

function selectorDays(selector) {
  if (selector === '--expire-local') {
    return options.expireLocalDays
  }
  if (selector === '--expire-closed') {
    return options.expireClosedDays
  }
  if (selector === '--expire-gone') {
    return options.expireGoneDays
  }
  if (selector === '--expire-archived') {
    return options.expireArchivedDays
  }
  return undefined
}

function defaultSelectorDays(selector) {
  if (selector === '--expire-local' || selector === '--expire-closed') {
    return DEFAULT_DAYS
  }
  if (selector === '--expire-gone') {
    return DEFAULT_GONE_DAYS
  }
  if (selector === '--expire-archived') {
    return DEFAULT_ARCHIVED_DAYS
  }
  return undefined
}

function effectiveSelectorDays(selector) {
  return selectorDays(selector) ?? defaultSelectorDays(selector)
}

let root = process.cwd()
const top = run('git', ['rev-parse', '--show-toplevel'], { allowFailure: true })
if (top.status !== 0) {
  fail(`not a git repository: ${top.stderr.trim() || root}`)
}
root = top.stdout.trim()

function run(command, args, { allowFailure = false } = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  })
  if (result.error) {
    throw new Error(`${command} ${args.join(' ')}: ${result.error.message}`)
  }
  if (result.status !== 0 && !allowFailure) {
    throw new Error(
      `${command} ${args.join(' ')} failed (${result.status}): ${(result.stderr || result.stdout).trim()}`,
    )
  }
  return result
}

if (run('git', ['rev-parse', '--verify', '--quiet', options.base], { allowFailure: true }).status !== 0) {
  fail(`base branch '${options.base}' does not exist`)
}

if (options.fetch) {
  process.stdout.write(`Fetching ${options.remote} (--prune)...\n`)
  const fetched = run('git', ['fetch', '--prune', options.remote], { allowFailure: true })
  if (fetched.status !== 0) {
    fail(`git fetch --prune ${options.remote} failed: ${(fetched.stderr || fetched.stdout).trim()}`)
  }
}

const repository = run(gh, ['repo', 'view', '--json', 'nameWithOwner', '--jq', '.nameWithOwner'], {
  allowFailure: true,
})
if (repository.status !== 0) {
  fail(
    `cannot read the GitHub repository with '${gh}': ${(repository.stderr || repository.stdout).trim()}\n` +
      'The open-PR protection and remote deletion decisions need an authenticated gh.',
  )
}
const repoName = repository.stdout.trim()

const prResult = run(
  gh,
  [
    'pr',
    'list',
    '--state',
    'all',
    '--limit',
    String(PR_LIMIT),
    '--json',
    'number,headRefName,headRefOid,state,mergedAt,closedAt,isCrossRepository,url',
  ],
  { allowFailure: true },
)
if (prResult.status !== 0) {
  fail(`gh pr list failed: ${(prResult.stderr || prResult.stdout).trim()}`)
}

let pullRequests
try {
  pullRequests = JSON.parse(prResult.stdout)
} catch (error) {
  fail(`gh pr list returned invalid JSON: ${error.message}`)
}

/** Latest PR per head branch, ignoring forks (their heads are not ours to delete). */
const latestPr = new Map()
/** Open PRs by head branch, used only for protection. */
const openPr = new Map()
for (const pr of pullRequests) {
  if (pr.isCrossRepository) {
    continue
  }
  if (pr.state === 'OPEN' && !openPr.has(pr.headRefName)) {
    openPr.set(pr.headRefName, pr)
  }
  const current = latestPr.get(pr.headRefName)
  if (current === undefined || pr.number > current.number) {
    latestPr.set(pr.headRefName, pr)
  }
}

/** Branch name -> reasons it must not be deleted. */
const protectedBranches = new Map()

function protect(branch, reason) {
  const reasons = protectedBranches.get(branch) ?? new Set()
  reasons.add(reason)
  protectedBranches.set(branch, reasons)
}

protect(options.base, 'base branch')
protect('main', 'default branch')

const defaultRef = run(
  'git',
  ['symbolic-ref', '--short', `refs/remotes/${options.remote}/HEAD`],
  { allowFailure: true },
)
if (defaultRef.status === 0) {
  const name = defaultRef.stdout.trim().replace(`${options.remote}/`, '')
  if (name !== '') {
    protect(name, 'default branch')
  }
}

const current = run('git', ['symbolic-ref', '--short', '-q', 'HEAD'], { allowFailure: true })
if (current.status === 0 && current.stdout.trim() !== '') {
  protect(current.stdout.trim(), 'current branch')
}

const worktrees = run('git', ['worktree', 'list', '--porcelain']).stdout.split('\n')
let worktreePath = ''
for (const line of worktrees) {
  if (line.startsWith('worktree ')) {
    worktreePath = line.slice('worktree '.length)
  } else if (line.startsWith('branch refs/heads/')) {
    protect(
      line.slice('branch refs/heads/'.length),
      `worktree ${worktreePath.split(/[\\/]/).pop()}`,
    )
  }
}

for (const [branch, pr] of openPr) {
  protect(branch, `open #${pr.number}`)
}

function globMatch(pattern, name) {
  const escaped = pattern
    .split('*')
    .map((part) => part.replace(/[.+?^${}()|[\]\\]/g, '\\$&'))
    .join('.*')
  return new RegExp(`^${escaped}$`).test(name)
}

function patternFor(name) {
  return options.protect.find((pattern) => globMatch(pattern, name))
}

// Local branches and the subset merged into the base branch.
const localBranches = run(
  'git',
  [
    'for-each-ref',
    '--format=%(refname:short)%09%(upstream:short)%09%(upstream:track)%09%(committerdate:unix)%09%(objectname)',
    'refs/heads',
  ],
).stdout
  .split('\n')
  .filter((line) => line !== '')
  .map((line) => {
    const [name, upstream, track, committed, oid] = line.split('\t')
    return {
      name,
      upstream,
      gone: track.includes('[gone]'),
      committed: Number(committed),
      oid,
    }
  })

const mergedLocal = new Set(
  run('git', ['for-each-ref', '--format=%(refname:short)', '--merged', options.base, 'refs/heads'])
    .stdout.split('\n')
    .filter((line) => line !== ''),
)

// Remote heads, as they exist on the remote right now.
const remoteHeads = run('git', ['ls-remote', '--heads', options.remote]).stdout
  .split('\n')
  .filter((line) => line !== '')
  .map((line) => {
    const [oid, ref] = line.split('\t')
    return { name: ref.replace(/^refs\/heads\//, ''), oid }
  })

// User patterns (backup/*, release/*, --protect) protect both local and remote
// occurrences of a name. Apply them before building the protected decisions.
for (const branch of [...localBranches, ...remoteHeads]) {
  if (protectedBranches.has(branch.name)) {
    continue
  }
  const pattern = patternFor(branch.name)
  if (pattern !== undefined) {
    protect(branch.name, `protected pattern ${pattern}`)
  }
}

const dayMs = 24 * 60 * 60 * 1000
const now = Date.now()
const localDays = options.expireLocalDays ?? DEFAULT_DAYS
const closedDays = options.expireClosedDays ?? DEFAULT_DAYS
const goneDays = options.expireGoneDays ?? DEFAULT_GONE_DAYS
const archivedDays = options.expireArchivedDays ?? DEFAULT_ARCHIVED_DAYS
const localCutoff = now - localDays * dayMs
const closedCutoff = now - closedDays * dayMs
const goneCutoff = now - goneDays * dayMs
const archivedCutoff = now - archivedDays * dayMs
const archiveDate = new Date(now).toISOString().slice(0, 10)
const archivePrefix = `refs/archive/${archiveDate}`

const existingArchives = run('git', ['for-each-ref', '--format=%(refname)', 'refs/archive'])
  .stdout.split('\n')
  .filter((ref) => ref !== '')
  .map((ref) => {
    const match = ref.match(/^refs\/archive\/(\d{4}-\d{2}-\d{2})\/(.+)$/)
    if (match === null) {
      return undefined
    }
    const at = Date.parse(`${match[1]}T00:00:00Z`)
    return Number.isFinite(at)
      ? { ref, date: match[1], branch: match[2], at }
      : undefined
  })
  .filter((entry) => entry !== undefined)

function ageDays(committed) {
  return Math.max(0, Math.floor((now - committed * 1000) / dayMs))
}

function commitsAhead(branch) {
  const result = run('git', ['rev-list', '--count', `${options.base}..${branch}`], {
    allowFailure: true,
  })
  return Number(result.stdout.trim()) || 0
}

function commitAgeDays(oid) {
  const result = run('git', ['show', '-s', '--format=%ct', oid], { allowFailure: true })
  if (result.status !== 0) {
    return undefined
  }
  const committed = Number(result.stdout.trim())
  return Number.isFinite(committed) ? ageDays(committed) : undefined
}

function prTiming(pr) {
  if (pr.state !== 'MERGED' && pr.state !== 'CLOSED') {
    return undefined
  }
  const kind = pr.state === 'MERGED' ? 'merged' : 'closed'
  const at = Date.parse(kind === 'merged' ? pr.mergedAt : pr.closedAt)
  return Number.isFinite(at) ? { kind, at } : undefined
}

function archiveFor(name) {
  return { ref: `${archivePrefix}/${name}` }
}

function decision(input) {
  return {
    scope: input.scope,
    name: input.name,
    display: input.display,
    category: input.category,
    action: input.action,
    result: input.action === 'keep' ? 'kept' : undefined,
    evidence: input.evidence ?? {},
    reason: input.reason ?? null,
    archive: input.archive ?? null,
    error: null,
  }
}

const decisions = []
const localByName = new Map(localBranches.map((branch) => [branch.name, branch]))
const remoteByName = new Map(remoteHeads.map((branch) => [branch.name, branch]))

for (const [name, reasons] of protectedBranches) {
  const local = localByName.get(name)
  const remote = remoteByName.get(name)
  decisions.push(
    decision({
      scope: 'protected',
      name,
      display: local ? `local ${name}` : remote ? `remote ${options.remote}/${name}` : name,
      category: 'protected',
      action: 'keep',
      reason: [...reasons].join(', '),
    }),
  )
}

for (const branch of localBranches) {
  if (protectedBranches.has(branch.name)) {
    continue
  }
  const base = { scope: 'local', name: branch.name, display: `local ${branch.name}` }

  if (mergedLocal.has(branch.name)) {
    decisions.push(
      decision({
        ...base,
        category: 'local.merged-base',
        action: 'prune',
      }),
    )
    continue
  }

  const pr = latestPr.get(branch.name)
  const timing = pr === undefined ? undefined : prTiming(pr)
  // A local branch whose already-closed PR no longer has a remote head is
  // upstream-gone even when no upstream tracking ref was configured.
  const closedExact =
    pr !== undefined && pr.headRefOid === branch.oid && timing?.kind === 'closed'
  const upstreamGone = branch.gone || (closedExact && !remoteByName.has(branch.name))
  if (upstreamGone) {
    const expired = branch.committed * 1000 < goneCutoff
    decisions.push(
      decision({
        ...base,
        category: 'local.gone',
        action: expired ? 'prune' : 'keep',
        evidence: { tipAgeDays: ageDays(branch.committed) },
        archive: archiveFor(branch.name),
      }),
    )
    continue
  }

  const exactPr =
    pr !== undefined && pr.headRefOid === branch.oid && timing !== undefined
  if (!exactPr) {
    const expired = branch.committed * 1000 < localCutoff
    decisions.push(
      decision({
        ...base,
        category: 'local.no-exact-pr',
        action: expired ? 'prune' : 'keep',
        evidence: {
          prNumber: pr?.number,
          prKind: pr === undefined ? undefined : (timing?.kind ?? 'unknown'),
          prAt: timing?.at ?? null,
          tipAgeDays: ageDays(branch.committed),
          commitsAhead: commitsAhead(branch.name),
          upstream: branch.upstream || null,
        },
        archive: archiveFor(branch.name),
      }),
    )
    continue
  }

  const stale = timing.at < closedCutoff
  decisions.push(
    decision({
      ...base,
      category: timing.kind === 'merged' ? 'local.pr-merged' : 'local.pr-closed',
      action: stale ? 'prune' : 'keep',
      evidence: { prNumber: pr.number, prKind: timing.kind, prAt: timing.at },
      archive: archiveFor(branch.name),
    }),
  )
}

for (const branch of remoteHeads) {
  if (protectedBranches.has(branch.name)) {
    continue
  }
  const base = {
    scope: 'remote',
    name: branch.name,
    display: `remote ${options.remote}/${branch.name}`,
  }
  const pr = latestPr.get(branch.name)
  const timing = pr === undefined ? undefined : prTiming(pr)
  const tipMismatch = pr !== undefined && pr.headRefOid !== branch.oid
  if (pr === undefined || tipMismatch || timing === undefined) {
    decisions.push(
      decision({
        ...base,
        category: 'remote.no-pr',
        action: 'keep',
        evidence: {
          tipAgeDays: commitAgeDays(branch.oid),
          source: tipMismatch ? 'tip mismatch' : pr === undefined ? 'no PR' : 'no merged PR',
        },
      }),
    )
    continue
  }
  const stale = timing.at < closedCutoff
  decisions.push(
    decision({
      ...base,
      category: timing.kind === 'merged' ? 'remote.pr-merged' : 'remote.pr-closed',
      action: stale ? 'prune' : 'keep',
      evidence: { prNumber: pr.number, prKind: timing.kind, prAt: timing.at },
    }),
  )
}

for (const entry of existingArchives) {
  const expired = entry.at < archivedCutoff
  decisions.push(
    decision({
      scope: 'archive',
      name: entry.ref,
      display: entry.ref,
      category: 'archived',
      action: expired ? 'prune' : 'keep',
      evidence: { archiveAt: entry.at, ref: entry.ref },
    }),
  )
}

function compareDecisions(left, right) {
  const category = CATEGORY_ORDER.get(left.category) - CATEGORY_ORDER.get(right.category)
  if (category !== 0) {
    return category
  }
  if (left.action !== right.action) {
    return left.action === 'prune' ? -1 : 1
  }
  const sort = CATEGORY_BY_ID.get(left.category).sort
  if (sort === 'pr') {
    const leftDay = Math.floor((left.evidence.prAt ?? 0) / dayMs)
    const rightDay = Math.floor((right.evidence.prAt ?? 0) / dayMs)
    if (leftDay !== rightDay) {
      return leftDay - rightDay
    }
  } else if (sort === 'tip') {
    const leftAge = left.evidence.tipAgeDays ?? 0
    const rightAge = right.evidence.tipAgeDays ?? 0
    if (leftAge !== rightAge) {
      return rightAge - leftAge
    }
  } else if (sort === 'archive') {
    const leftDay = Math.floor((left.evidence.archiveAt ?? 0) / dayMs)
    const rightDay = Math.floor((right.evidence.archiveAt ?? 0) / dayMs)
    if (leftDay !== rightDay) {
      return leftDay - rightDay
    }
  }
  return left.display.localeCompare(right.display)
}

const sorted = [...decisions].sort(compareDecisions)

function isAncestor(branch) {
  return (
    run('git', ['merge-base', '--is-ancestor', `refs/heads/${branch}`, options.base], {
      allowFailure: true,
    }).status === 0
  )
}

function deleteLocal(branch, { force = false } = {}) {
  let result = run('git', ['branch', force ? '-D' : '-d', branch], { allowFailure: true })
  // `-d` compares against the upstream when one is configured, even though the
  // branch is merged into the base branch. Re-check the merge before forcing.
  if (result.status !== 0 && !force && isAncestor(branch)) {
    result = run('git', ['branch', '-D', branch], { allowFailure: true })
  }
  return result.status === 0 ? undefined : (result.stderr || result.stdout).trim()
}

function archiveLocal(decisionToArchive) {
  if (decisionToArchive.archive === null) {
    return undefined
  }
  const result = run(
    'git',
    ['update-ref', decisionToArchive.archive.ref, `refs/heads/${decisionToArchive.name}`],
    { allowFailure: true },
  )
  return result.status === 0 ? undefined : (result.stderr || result.stdout).trim()
}

function deleteRemote(branch) {
  const result = run('git', ['push', options.remote, '--delete', branch], { allowFailure: true })
  if (result.status === 0) {
    return undefined
  }
  const message = (result.stderr || result.stdout).trim()
  // A concurrent deletion is the outcome we wanted.
  return message.includes('remote ref does not exist') ? undefined : message
}

function execute() {
  for (const decisionToApply of sorted) {
    if (decisionToApply.action === 'keep') {
      decisionToApply.result = 'kept'
      continue
    }
    if (!options.apply) {
      decisionToApply.result =
        options.archive && decisionToApply.archive !== null
          ? 'would-delete+archive'
          : 'would-delete'
      continue
    }

    if (decisionToApply.scope === 'archive') {
      const result = run('git', ['update-ref', '-d', decisionToApply.evidence.ref], {
        allowFailure: true,
      })
      if (result.status !== 0) {
        decisionToApply.result = 'failed'
        decisionToApply.error = (result.stderr || result.stdout).trim()
        continue
      }
      decisionToApply.result = 'deleted'
      continue
    }

    let archived = false
    if (decisionToApply.scope === 'local' && options.archive && decisionToApply.archive !== null) {
      const error = archiveLocal(decisionToApply)
      if (error !== undefined) {
        decisionToApply.result = 'failed'
        decisionToApply.error = `archive failed: ${error}`
        continue
      }
      archived = true
    }

    const error =
      decisionToApply.scope === 'remote'
        ? deleteRemote(decisionToApply.name)
        : deleteLocal(decisionToApply.name, {
            force: decisionToApply.category !== 'local.merged-base',
          })
    if (error !== undefined) {
      decisionToApply.result = 'failed'
      decisionToApply.error = error
      continue
    }
    decisionToApply.result =
      decisionToApply.scope === 'remote'
        ? 'deleted'
        : archived
          ? 'deleted+archived'
          : 'deleted'
  }
}

execute()

function pad(text, width) {
  return text.length >= width ? text : text.padEnd(width)
}

function padStart(text, width) {
  return text.length >= width ? text : text.padStart(width)
}

function optionLabel(selector) {
  if (selector === null) {
    return '—'
  }
  return `${selector} ${effectiveSelectorDays(selector)}`
}

const STATUS_LABELS = {
  kept: '[keep]',
  'would-delete': '[delete]',
  'would-delete+archive': '[archive]',
  deleted: '[deleted]',
  'deleted+archived': '[archive]',
}

function statusLabel(decisionToRender) {
  if (decisionToRender.result === 'failed') {
    return '[failed]'
  }
  return STATUS_LABELS[decisionToRender.result] ?? `[${decisionToRender.result}]`
}

function prAgeLabel(evidence) {
  if (evidence.prAt === null || evidence.prAt === undefined) {
    return 'unknown age'
  }
  const days = Math.max(0, Math.floor((now - evidence.prAt) / dayMs))
  return `${days} ${days === 1 ? 'day' : 'days'}`
}

function prLabel(evidence) {
  const age = prAgeLabel(evidence)
  return evidence.prKind === undefined || evidence.prKind === 'unknown'
    ? `#${evidence.prNumber}, ${age}`
    : `#${evidence.prNumber}, ${age} ${evidence.prKind}`
}

function detail(decisionToRender) {
  switch (decisionToRender.category) {
    case 'protected':
      return ''
    case 'local.merged-base':
      return 'merged into main'
    case 'local.pr-merged':
    case 'local.pr-closed':
    case 'remote.pr-merged':
    case 'remote.pr-closed':
      return prLabel(decisionToRender.evidence)
    case 'local.gone':
      return `tip ${decisionToRender.evidence.tipAgeDays}d`
    default:
      return ''
  }
}

function columnsFor(decisionToRender) {
  if (decisionToRender.category === 'remote.no-pr') {
    return {
      tip:
        decisionToRender.evidence.tipAgeDays === undefined
          ? 'tip unknown'
          : `tip ${decisionToRender.evidence.tipAgeDays}d`,
      source: decisionToRender.evidence.source,
    }
  }
  if (decisionToRender.category !== 'local.no-exact-pr') {
    return undefined
  }
  return {
    tip: `tip ${decisionToRender.evidence.tipAgeDays}d`,
    commits: `+${decisionToRender.evidence.commitsAhead}`,
    source:
      decisionToRender.evidence.prNumber !== undefined
        ? prLabel(decisionToRender.evidence)
        : (decisionToRender.evidence.upstream ?? 'no upstream'),
  }
}

function formatDecision(decisionToRender, nameWidth, statusWidth, columns, tipWidth, commitWidth) {
  const status = statusLabel(decisionToRender)
  if (columns !== undefined) {
    const parts = [
      pad(decisionToRender.display, nameWidth),
      pad(status, statusWidth),
      pad(columns.tip, tipWidth),
    ]
    if (columns.commits !== undefined) {
      parts.push(padStart(columns.commits, commitWidth))
    }
    parts.push(columns.source)
    return parts.join('  ')
  }
  const text = detail(decisionToRender)
  const reason =
    decisionToRender.category === 'protected' && decisionToRender.reason !== null
      ? decisionToRender.reason
      : ''
  const tail = [text, reason].filter((part) => part !== '').join('  ')
  const prefix = `${pad(decisionToRender.display, nameWidth)}  ${pad(status, statusWidth)}`
  return tail === '' ? prefix.trimEnd() : `${prefix}  ${tail}`
}

function sectionTitle(category, kept, prune) {
  if (prune > 0 && kept > 0) {
    return `${category.title} — keep ${kept}, prune ${prune}`
  }
  if (prune > 0) {
    return `${category.title} — prune ${prune}`
  }
  return `${category.title} — keep ${kept}`
}

function renderHeader() {
  const mode = options.apply ? 'apply' : 'dry run'
  return [
    `Repository ${repoName} — remote ${options.remote}, base ${options.base}`,
    `Mode: ${mode}`,
    `Cutoffs: local ${localDays}d, closed ${closedDays}d, gone ${goneDays}d, archived ${archivedDays}d`,
  ]
}

function renderOverview(decisionsToRender) {
  const lines = []
  const rows = CATEGORIES.map((category) => {
    const items = decisionsToRender.filter(
      (decisionToRender) => decisionToRender.category === category.id,
    )
    const kept = items.filter((decisionToRender) => decisionToRender.action === 'keep').length
    const prune = items.filter((decisionToRender) => decisionToRender.action === 'prune').length
    return {
      title: category.title,
      kept,
      prune: category.selector === null ? '—' : String(prune),
      option: optionLabel(category.selector),
      pruneCount: prune,
    }
  }).filter((row) => row.kept + row.pruneCount > 0)

  const keptWidth = Math.max('KEEP'.length, ...rows.map((row) => String(row.kept).length))
  const pruneWidth = Math.max('PRUNE'.length, ...rows.map((row) => row.prune.length))
  const titleWidth = Math.max('CATEGORY'.length, ...rows.map((row) => row.title.length))
  lines.push('')
  lines.push('Overview')
  lines.push(
    `${pad('CATEGORY', titleWidth)}  ${padStart('KEEP', keptWidth)}  ${padStart('PRUNE', pruneWidth)}  OPTION`,
  )
  for (const row of rows) {
    lines.push(
      `${pad(row.title, titleWidth)}  ${padStart(String(row.kept), keptWidth)}  ${padStart(row.prune, pruneWidth)}  ${row.option}`,
    )
  }
  const totalKept = rows.reduce((total, row) => total + row.kept, 0)
  const totalPrune = rows.reduce((total, row) => total + row.pruneCount, 0)
  lines.push(
    `${pad('TOTAL', titleWidth)}  ${padStart(String(totalKept), keptWidth)}  ${padStart(String(totalPrune), pruneWidth)}  `,
  )
  return lines
}

function renderSections(decisionsToRender) {
  const lines = []
  for (const category of CATEGORIES) {
    const items = decisionsToRender.filter(
      (decisionToRender) => decisionToRender.category === category.id,
    )
    if (items.length === 0) {
      continue
    }
    const kept = items.filter((decisionToRender) => decisionToRender.action === 'keep').length
    const prune = items.filter((decisionToRender) => decisionToRender.action === 'prune').length
    const nameWidth = Math.max(52, ...items.map((item) => item.display.length))
    const statusWidth = Math.max(...items.map((item) => statusLabel(item).length))
    const columns = items.map(columnsFor)
    const columnMode = columns.every((column) => column !== undefined)
    const tipWidth = columnMode ? Math.max(...columns.map((column) => column.tip.length)) : 0
    const commitWidth = columnMode
      ? Math.max(0, ...columns.map((column) => column.commits?.length ?? 0))
      : 0
    lines.push('')
    const option = category.selector === null ? '' : ` (${optionLabel(category.selector)})`
    lines.push(`${sectionTitle(category, kept, prune)}${option}`)
    for (let index = 0; index < items.length; index += 1) {
      lines.push(
        `  ${formatDecision(items[index], nameWidth, statusWidth, columns[index], tipWidth, commitWidth)}`,
      )
    }
  }
  return lines
}

function renderFooter(decisionsToRender) {
  const lines = []
  const failures = decisionsToRender.filter(
    (decisionToRender) => decisionToRender.result === 'failed',
  )
  if (failures.length > 0) {
    lines.push('')
    lines.push(`Failures (${failures.length}):`)
    for (const failure of failures) {
      lines.push(`  ${failure.scope} ${failure.name}: ${failure.error}`)
    }
    return lines
  }

  if (options.apply) {
    const deletedLocal = decisionsToRender.filter(
      (decisionToRender) =>
        decisionToRender.scope === 'local' &&
        decisionToRender.result !== undefined &&
        decisionToRender.result.startsWith('deleted'),
    ).length
    const deletedRemote = decisionsToRender.filter(
      (decisionToRender) =>
        decisionToRender.scope === 'remote' &&
        decisionToRender.result !== undefined &&
        decisionToRender.result.startsWith('deleted'),
    ).length
    const archived = decisionsToRender.filter(
      (decisionToRender) => decisionToRender.result === 'deleted+archived',
    ).length
    const deletedArchived = decisionsToRender.filter(
      (decisionToRender) =>
        decisionToRender.scope === 'archive' && decisionToRender.result === 'deleted',
    ).length
    lines.push('')
    lines.push(`Deleted ${deletedLocal} local and ${deletedRemote} remote branches.`)
    if (deletedArchived > 0) {
      lines.push(
        `Deleted ${deletedArchived} archived ref${deletedArchived === 1 ? '' : 's'}.`,
      )
    }
    if (archived > 0) {
      lines.push(
        `Archived ${archived} local branches under ${archivePrefix}/ (restore with git branch <name> <ref>).`,
      )
    }
    return lines
  }

  const plannedLocal = decisionsToRender.filter(
    (decisionToRender) =>
      decisionToRender.scope === 'local' &&
      decisionToRender.category !== 'local.gone' &&
      decisionToRender.action === 'prune',
  ).length
  const plannedGone = decisionsToRender.filter(
    (decisionToRender) =>
      decisionToRender.category === 'local.gone' && decisionToRender.action === 'prune',
  ).length
  const plannedRemote = decisionsToRender.filter(
    (decisionToRender) =>
      decisionToRender.scope === 'remote' && decisionToRender.action === 'prune',
  ).length
  const plannedArchived = decisionsToRender.filter(
    (decisionToRender) =>
      decisionToRender.category === 'archived' && decisionToRender.action === 'prune',
  ).length
  const archivedPart =
    plannedArchived > 0
      ? `; ${plannedArchived} archived ref${plannedArchived === 1 ? '' : 's'} would be deleted`
      : ' would be deleted'
  lines.push('')
  lines.push(
    `Dry run: ${plannedLocal} local, ${plannedGone} gone-upstream and ${plannedRemote} remote branches${archivedPart}. Re-run with --apply.`,
  )
  return lines
}

function render(decisionsToRender) {
  return [
    ...renderHeader(),
    ...renderOverview(decisionsToRender),
    ...renderSections(decisionsToRender),
    ...renderFooter(decisionsToRender),
  ].join('\n') + '\n'
}

if (decisions.some((decisionToRender) => decisionToRender.result === 'failed')) {
  process.exitCode = 1
}

process.stdout.write(render(sorted))
