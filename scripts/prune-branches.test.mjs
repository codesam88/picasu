#!/usr/bin/env node
// Fixture-repo tests for prune-branches.mjs. No network: the remote is a local
// bare repository and `gh` is a stub that serves a canned PR list.
import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'

const script = join(dirname(fileURLToPath(import.meta.url)), 'prune-branches.mjs')

function git(cwd, args, env) {
  execFileSync('git', args, {
    cwd,
    stdio: 'ignore',
    env: env === undefined ? process.env : { ...process.env, ...env },
  })
}

function sha(cwd, ref) {
  return execFileSync('git', ['rev-parse', ref], { cwd, encoding: 'utf8' }).trim()
}

function hasLocal(repo, branch) {
  return (
    spawnSync('git', ['show-ref', '--verify', '--quiet', `refs/heads/${branch}`], { cwd: repo })
      .status === 0
  )
}

function hasRemote(remote, branch) {
  return (
    spawnSync('git', ['ls-remote', '--heads', remote, `refs/heads/${branch}`], {
      encoding: 'utf8',
    }).stdout.trim() !== ''
  )
}

function archiveRefs(repo) {
  return execFileSync('git', ['for-each-ref', '--format=%(refname)', 'refs/archive'], {
    cwd: repo,
    encoding: 'utf8',
  })
    .trim()
    .split('\n')
    .filter((line) => line !== '')
}

/**
 * Build a repository with one branch per deletion category:
 * merged-old (merged local + remote), closed-old (closed remote), recent
 * (merged inside the cutoff), open-branch (open PR), backup/keep (pattern),
 * worktree-branch (checked out), unmerged (no PR), gone-unmerged (upstream
 * gone).
 */
function fixture(t) {
  const work = mkdtempSync(join(tmpdir(), 'prune-branches-'))
  t.after(() => rmSync(work, { recursive: true, force: true }))
  const repo = join(work, 'repo')
  const remote = join(work, 'origin.git')
  const bin = join(work, 'bin')

  mkdirSync(repo)
  git(work, ['init', '-q', '--bare', '-b', 'main', remote])
  git(work, ['init', '-q', '-b', 'main', repo])
  git(repo, ['config', 'user.email', 'test@example.com'])
  git(repo, ['config', 'user.name', 'Test'])

  writeFileSync(join(repo, 'a.txt'), 'a\n')
  git(repo, ['add', '.'])
  git(repo, ['commit', '-qm', 'A'])
  git(repo, ['remote', 'add', 'origin', remote])
  git(repo, ['push', '-q', '-u', 'origin', 'main'])

  git(repo, ['branch', 'merged-old'])
  git(repo, ['branch', 'backup/keep'])
  git(repo, ['branch', 'worktree-branch'])
  git(repo, ['push', '-q', 'origin', 'merged-old', 'backup/keep', 'worktree-branch'])

  const oids = {
    'merged-old': sha(repo, 'merged-old'),
    'worktree-branch': sha(repo, 'worktree-branch'),
  }

  for (const branch of ['unmerged', 'closed-old', 'recent', 'open-branch']) {
    git(repo, ['checkout', '-qb', branch, 'main'])
    writeFileSync(join(repo, `${branch}.txt`), `${branch}\n`)
    git(repo, ['add', '.'])
    git(
      repo,
      ['commit', '-qm', branch],
      branch === 'closed-old'
        ? {
            GIT_AUTHOR_DATE: '2020-01-01T00:00:00Z',
            GIT_COMMITTER_DATE: '2020-01-01T00:00:00Z',
          }
        : undefined,
    )
    git(repo, ['push', '-q', 'origin', branch])
    oids[branch] = sha(repo, branch)
    git(repo, ['checkout', '-q', 'main'])
  }

  // reused: a merged PR's head branch that received another commit afterwards.
  // The stale PR must not authorise deleting the new tip.
  git(repo, ['checkout', '-qb', 'reused', 'main'])
  writeFileSync(join(repo, 'reused.txt'), 'reused\n')
  git(repo, ['add', '.'])
  git(repo, ['commit', '-qm', 'reused'])
  git(repo, ['push', '-q', 'origin', 'reused'])
  oids['reused'] = sha(repo, 'reused')
  writeFileSync(join(repo, 'reused.txt'), 'reused again\n')
  git(repo, ['commit', '-qam', 'reused again'])
  git(repo, ['push', '-q', 'origin', 'reused'])
  git(repo, ['checkout', '-q', 'main'])

  // squash-merged: not merged into main, but a stale merged PR records the
  // exact current tip (squash/rebase result).
  git(repo, ['checkout', '-qb', 'squash-merged', 'main'])
  writeFileSync(join(repo, 'squash-merged.txt'), 'squash\n')
  git(repo, ['add', '.'])
  git(repo, ['commit', '-qm', 'squash merged'])
  oids['squash-merged'] = sha(repo, 'squash-merged')
  git(repo, ['checkout', '-q', 'main'])

  // changed-after-pr: the stale PR records an older tip, so the newer local
  // commit must keep the branch.
  git(repo, ['checkout', '-qb', 'changed-after-pr', 'main'])
  writeFileSync(join(repo, 'changed.txt'), 'first\n')
  git(repo, ['add', '.'])
  git(repo, ['commit', '-qm', 'changed first'])
  oids['changed-after-pr'] = sha(repo, 'changed-after-pr')
  writeFileSync(join(repo, 'changed.txt'), 'second\n')
  git(repo, ['commit', '-qam', 'changed second'])
  git(repo, ['checkout', '-q', 'main'])

  git(repo, ['checkout', '-qb', 'gone-unmerged', 'main'])
  writeFileSync(join(repo, 'gone.txt'), 'gone\n')
  git(repo, ['add', '.'])
  git(repo, ['commit', '-qm', 'gone'], {
    GIT_AUTHOR_DATE: '2020-01-01T00:00:00Z',
    GIT_COMMITTER_DATE: '2020-01-01T00:00:00Z',
  })
  git(repo, ['push', '-q', '-u', 'origin', 'gone-unmerged'])
  git(repo, ['checkout', '-q', 'main'])
  git(repo, ['push', '-q', 'origin', '--delete', 'gone-unmerged'])
  git(repo, ['fetch', '-q', '--prune', 'origin'])

  git(repo, ['worktree', 'add', '-q', join(work, 'wt'), 'worktree-branch'])

  mkdirSync(bin)
  writeFileSync(
    join(bin, 'gh'),
    '#!/bin/sh\nset -e\ncase "$1 $2" in\n' +
      '  "repo view") echo test/repo ;;\n' +
      '  "pr list") cat "$GH_PR_JSON" ;;\n' +
      '  *) exit 1 ;;\n' +
      'esac\n',
  )
  chmodSync(join(bin, 'gh'), 0o755)

  const now = new Date().toISOString()
  const prFile = join(work, 'prs.json')
  writeFileSync(
    prFile,
    JSON.stringify([
      pr(1, 'merged-old', oids['merged-old'], 'MERGED', '2020-01-01T00:00:00Z', null),
      pr(2, 'closed-old', oids['closed-old'], 'CLOSED', null, '2020-01-01T00:00:00Z'),
      pr(3, 'recent', oids['recent'], 'MERGED', now, null),
      pr(4, 'open-branch', oids['open-branch'], 'OPEN', null, null),
      pr(5, 'worktree-branch', oids['worktree-branch'], 'MERGED', '2020-01-01T00:00:00Z', null),
      pr(6, 'reused', oids['reused'], 'MERGED', '2020-01-01T00:00:00Z', null),
      pr(7, 'squash-merged', oids['squash-merged'], 'MERGED', '2020-01-01T00:00:00Z', null),
      pr(
        8,
        'changed-after-pr',
        oids['changed-after-pr'],
        'MERGED',
        '2020-01-01T00:00:00Z',
        null,
      ),
    ]),
  )

  return {
    repo,
    remote,
    run: (args) =>
      spawnSync(process.execPath, [script, '--no-fetch', ...args], {
        cwd: repo,
        encoding: 'utf8',
        env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, GH_PR_JSON: prFile },
      }),
  }
}

function pr(number, headRefName, headRefOid, state, mergedAt, closedAt) {
  return {
    number,
    headRefName,
    headRefOid,
    state,
    mergedAt,
    closedAt,
    isCrossRepository: false,
    url: `https://example.test/pull/${number}`,
  }
}

test('dry run marks the default cutoffs', (t) => {
  const f = fixture(t)
  const result = f.run([])
  const out = `${result.stdout}${result.stderr}`

  assert.equal(result.status, 0, out)
  assert.match(out, /Protected — keep 4/)
  assert.match(out, /local backup\/keep\s+\[keep\]\s+protected pattern backup\/\*/)
  assert.match(out, /local open-branch\s+\[keep\]\s+open #4/)
  assert.match(out, /local merged-old\s+\[delete\]\s+merged into main/)
  assert.match(out, /local squash-merged\s+\[archive\]\s+#7, \d+ days merged/)
  assert.match(out, /local closed-old\s+\[archive\]\s+#2, \d+ days closed/)
  assert.match(out, /Local merged into base — prune 1 \(--expire-local 14\)/)
  assert.match(out, /Local PR merged — keep 1, prune 1 \(--expire-closed 14\)/)
  assert.match(out, /Local PR closed — prune 1 \(--expire-closed 14\)/)
  assert.match(out, /Upstream gone — prune 1 \(--expire-gone 7\)/)
  assert.match(out, /Local no exact PR — keep 3 \(--expire-local 14\)/)
  assert.match(out, /Remote PR merged — keep 1, prune 1 \(--expire-closed 14\)/)
  assert.match(out, /Remote PR closed — prune 1 \(--expire-closed 14\)/)
  assert.match(out, /Remote no PR — keep 2/)
  assert.match(out, /remote origin\/merged-old\s+\[delete\]\s+#1, \d+ days merged/)
  assert.match(out, /remote origin\/recent\s+\[keep\]\s+#3, \d+ days merged/)
  assert.match(out, /remote origin\/closed-old\s+\[delete\]\s+#2, \d+ days closed/)
  assert.match(out, /remote origin\/reused\s+\[keep\]\s+tip \d+d\s+tip mismatch/)
  assert.match(out, /remote origin\/unmerged\s+\[keep\]\s+tip \d+d\s+no PR/)
  assert.match(out, /changed-after-pr\s+\[keep\]\s+tip \d+d\s+\+2\s+#8, \d+ days merged/)
  assert.match(out, /reused\s+\[keep\]\s+tip \d+d\s+\+2\s+#6, \d+ days merged/)
  assert.match(out, /unmerged\s+\[keep\]\s+tip \d+d\s+\+1\s+no upstream/)
  assert.match(out, /Cutoffs: local 14d, closed 14d, gone 7d, archived 30d/)
  assert.match(out, /TOTAL\s+11\s+6/)
  assert.match(out, /Local PR merged\s+1\s+1\s+--expire-closed 14/)
  assert.match(out, /Dry run: 3 local, 1 gone-upstream and 2 remote branches would be deleted\./)

  assert.equal(hasLocal(f.repo, 'merged-old'), true)
  assert.equal(hasRemote(f.remote, 'merged-old'), true)
})

test('apply uses the default cutoffs and archives unmerged branches', (t) => {
  const f = fixture(t)
  const result = f.run(['--apply'])
  const out = `${result.stdout}${result.stderr}`

  assert.equal(result.status, 0, out)
  assert.match(out, /Deleted 4 local and 2 remote branches\./)
  assert.match(out, /Archived 3 local branches under refs\/archive\//)

  for (const branch of ['merged-old', 'squash-merged', 'closed-old', 'gone-unmerged']) {
    assert.equal(hasLocal(f.repo, branch), false, `local ${branch}`)
  }
  for (const branch of ['merged-old', 'closed-old']) {
    assert.equal(hasRemote(f.remote, branch), false, `remote ${branch}`)
  }

  const archived = archiveRefs(f.repo)
  for (const branch of ['squash-merged', 'closed-old', 'gone-unmerged']) {
    assert.ok(archived.some((ref) => ref.endsWith(`/${branch}`)), archived.join('\n'))
  }
  assert.ok(!archived.some((ref) => ref.endsWith('/merged-old')), archived.join('\n'))

  for (const branch of [
    'backup/keep',
    'worktree-branch',
    'unmerged',
    'recent',
    'open-branch',
    'reused',
    'changed-after-pr',
  ]) {
    assert.equal(hasLocal(f.repo, branch), true, `local ${branch}`)
  }
  for (const branch of ['backup/keep', 'worktree-branch', 'unmerged', 'recent', 'open-branch', 'reused']) {
    assert.equal(hasRemote(f.remote, branch), true, `remote ${branch}`)
  }
})

test('expire-local override prunes changed-on-top-of-PR branches by tip age', (t) => {
  const f = fixture(t)
  const result = f.run([
    '--apply',
    '--expire-local',
    '0',
    '--expire-closed',
    '36500',
    '--expire-gone',
    '36500',
  ])
  const out = `${result.stdout}${result.stderr}`
  assert.equal(result.status, 0, out)
  assert.match(out, /Deleted 4 local and 0 remote branches\./)
  assert.match(out, /Local no exact PR — prune 3 \(--expire-local 0\)/)
  assert.equal(hasLocal(f.repo, 'changed-after-pr'), false)
  assert.equal(hasLocal(f.repo, 'reused'), false)
  assert.ok(archiveRefs(f.repo).some((ref) => ref.endsWith('/changed-after-pr')))
})

test('expire-local override prunes stale no-PR branches by tip age', (t) => {
  const f = fixture(t)
  git(f.repo, ['checkout', '-qb', 'stale-no-pr', 'main'])
  writeFileSync(join(f.repo, 'stale-no-pr.txt'), 'stale\n')
  git(f.repo, ['add', '.'])
  git(f.repo, ['commit', '-qm', 'stale no pr'], {
    GIT_AUTHOR_DATE: '2020-01-01T00:00:00Z',
    GIT_COMMITTER_DATE: '2020-01-01T00:00:00Z',
  })
  git(f.repo, ['checkout', '-q', 'main'])

  const plan = f.run([
    '--expire-local',
    '14',
    '--expire-closed',
    '36500',
    '--expire-gone',
    '36500',
  ])
  const planOut = `${plan.stdout}${plan.stderr}`
  assert.equal(plan.status, 0, planOut)
  assert.match(planOut, /Local no exact PR — keep 3, prune 1 \(--expire-local 14\)/)
  assert.match(planOut, /stale-no-pr\s+\[archive\]\s+tip \d+d/)

  const applied = f.run([
    '--apply',
    '--expire-local',
    '14',
    '--expire-closed',
    '36500',
    '--expire-gone',
    '36500',
  ])
  assert.equal(applied.status, 0)
  assert.match(`${applied.stdout}${applied.stderr}`, /Deleted 2 local and 0 remote branches\./)
  assert.equal(hasLocal(f.repo, 'stale-no-pr'), false)
  assert.ok(archiveRefs(f.repo).some((ref) => ref.endsWith('/stale-no-pr')))
})

test('expire-closed override prunes local and remote PR branches', (t) => {
  const f = fixture(t)
  const result = f.run([
    '--apply',
    '--expire-local',
    '36500',
    '--expire-closed',
    '14',
    '--expire-gone',
    '36500',
  ])
  const out = `${result.stdout}${result.stderr}`
  assert.equal(result.status, 0, out)
  assert.match(out, /Deleted 3 local and 2 remote branches\./)
  assert.equal(hasLocal(f.repo, 'squash-merged'), false)
  assert.equal(hasLocal(f.repo, 'closed-old'), false)
  assert.equal(hasLocal(f.repo, 'unmerged'), true)
  assert.equal(hasRemote(f.remote, 'merged-old'), false)
  assert.equal(hasRemote(f.remote, 'closed-old'), false)
})

test('local/closed overrides do not disable the gone default', (t) => {
  const f = fixture(t)
  const result = f.run([
    '--apply',
    '--expire-local',
    '36500',
    '--expire-closed',
    '36500',
  ])
  const out = `${result.stdout}${result.stderr}`
  assert.equal(result.status, 0, out)
  assert.match(out, /Deleted 2 local and 0 remote branches\./)
  assert.equal(hasLocal(f.repo, 'gone-unmerged'), false)
  assert.equal(hasLocal(f.repo, 'squash-merged'), true)
  assert.equal(hasLocal(f.repo, 'closed-old'), true)
  assert.ok(archiveRefs(f.repo).some((ref) => ref.endsWith('/gone-unmerged')))
})

test('expire-gone override prunes only gone-upstream branches older than DAYS', (t) => {
  const f = fixture(t)

  const plan = f.run([
    '--expire-gone',
    '0',
    '--expire-local',
    '36500',
    '--expire-closed',
    '36500',
  ])
  const planOut = `${plan.stdout}${plan.stderr}`
  assert.equal(plan.status, 0, planOut)
  assert.match(planOut, /Upstream gone — prune 1 \(--expire-gone 0\)/)
  assert.match(planOut, /gone-unmerged\s+\[archive\]\s+tip \d+d/)
  assert.match(planOut, /Dry run: 1 local, 1 gone-upstream and 0 remote branches would be deleted\./)

  const recent = f.run([
    '--apply',
    '--expire-gone',
    '36500',
    '--expire-local',
    '36500',
    '--expire-closed',
    '36500',
  ])
  assert.equal(recent.status, 0)
  assert.match(`${recent.stdout}${recent.stderr}`, /Deleted 1 local and 0 remote branches\./)
  assert.equal(hasLocal(f.repo, 'gone-unmerged'), true)

  const expired = f.run([
    '--apply',
    '--expire-gone',
    '0',
    '--expire-local',
    '36500',
    '--expire-closed',
    '36500',
  ])
  assert.equal(expired.status, 0)
  assert.match(`${expired.stdout}${expired.stderr}`, /Deleted 1 local and 0 remote branches\./)
  assert.equal(hasLocal(f.repo, 'gone-unmerged'), false)
  assert.ok(archiveRefs(f.repo).some((ref) => ref.endsWith('/gone-unmerged')))
})

test('expire-archived override deletes archive refs older than DAYS', (t) => {
  const f = fixture(t)
  const today = new Date().toISOString().slice(0, 10)
  git(f.repo, ['update-ref', `refs/archive/${today}/recent-archive`, 'refs/heads/main'])
  git(f.repo, ['update-ref', 'refs/archive/2020-01-01/old-archive', 'refs/heads/main'])

  const report = f.run([])
  const reportOut = `${report.stdout}${report.stderr}`
  assert.equal(report.status, 0, reportOut)
  assert.match(reportOut, /Cutoffs: local 14d, closed 14d, gone 7d, archived 30d/)
  assert.match(reportOut, /Archived — keep 1, prune 1 \(--expire-archived 30\)/)
  assert.match(reportOut, /refs\/archive\/2020-01-01\/old-archive\s+\[delete\]/)
  assert.match(reportOut, /refs\/archive\/\d{4}-\d{2}-\d{2}\/recent-archive\s+\[keep\]/)

  const plan = f.run(['--expire-archived', '30'])
  const planOut = `${plan.stdout}${plan.stderr}`
  assert.equal(plan.status, 0, planOut)
  assert.match(planOut, /Archived — keep 1, prune 1 \(--expire-archived 30\)/)

  const applied = f.run([
    '--apply',
    '--expire-local',
    '36500',
    '--expire-closed',
    '36500',
    '--expire-gone',
    '36500',
    '--expire-archived',
    '30',
  ])
  const appliedOut = `${applied.stdout}${applied.stderr}`
  assert.equal(applied.status, 0, appliedOut)
  assert.match(appliedOut, /Deleted 1 local and 0 remote branches\./)
  assert.match(appliedOut, /Deleted 1 archived ref\./)
  const refs = archiveRefs(f.repo)
  assert.ok(!refs.some((ref) => ref.endsWith('/old-archive')), refs.join('\n'))
  assert.ok(refs.some((ref) => ref.endsWith('/recent-archive')), refs.join('\n'))
})

test('local closed-PR branches become gone when the remote head is absent', (t) => {
  const f = fixture(t)
  git(f.repo, ['push', '-q', 'origin', '--delete', 'closed-old'])
  assert.equal(hasRemote(f.remote, 'closed-old'), false)
  assert.equal(hasLocal(f.repo, 'closed-old'), true)

  const after = f.run([])
  const out = `${after.stdout}${after.stderr}`
  assert.equal(after.status, 0, out)
  assert.match(out, /Upstream gone — prune 2 \(--expire-gone 7\)/)
  assert.match(out, /local closed-old\s+\[archive\]\s+tip \d+d/)
  assert.doesNotMatch(out, /Local PR closed —/)

  const expired = f.run([
    '--apply',
    '--expire-local',
    '36500',
    '--expire-closed',
    '36500',
    '--expire-gone',
    '7',
  ])
  const expiredOut = `${expired.stdout}${expired.stderr}`
  assert.equal(expired.status, 0, expiredOut)
  assert.match(expiredOut, /Deleted 3 local and 0 remote branches\./)
  assert.equal(hasLocal(f.repo, 'closed-old'), false)
  assert.ok(archiveRefs(f.repo).some((ref) => ref.endsWith('/closed-old')))
})
