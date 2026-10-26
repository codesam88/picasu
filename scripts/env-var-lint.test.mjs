#!/usr/bin/env node
// Behavior pins for env-var-lint.mjs. The extraction rules are the gate:
// a mention in a message or comment must not count as a read, README must
// not satisfy documentation, and a name the lint cannot verify must fail
// closed. Each case runs the lint against a throwaway fixture tree.
//
// Run: node --test scripts/env-var-lint.test.mjs
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'

const lint = join(dirname(fileURLToPath(import.meta.url)), 'env-var-lint.mjs')

const tableHeader = '# Config\n\n| Variable | Meaning |\n| --- | --- |\n'
const tableRow = (name) => `| \`${name}\` | documented |\n`

function fixture({ rs = '', config = tableHeader, readme = '' }) {
  const dir = mkdtempSync(join(tmpdir(), 'envlint-'))
  mkdirSync(join(dir, 'backend', 'src'), { recursive: true })
  mkdirSync(join(dir, 'docs'), { recursive: true })
  writeFileSync(join(dir, 'backend', 'src', 'lib.rs'), rs)
  writeFileSync(join(dir, 'docs', 'config.md'), config)
  writeFileSync(join(dir, 'README.md'), readme)
  return dir
}

function run(dir) {
  try {
    const out = execFileSync('node', [lint, dir], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    return { code: 0, out }
  } catch (error) {
    return {
      code: error.status ?? 1,
      out: `${error.stdout ?? ''}${error.stderr ?? ''}`,
    }
  }
}

function withFixture(t, files) {
  const dir = fixture(files)
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  return run(dir)
}

test('direct env::var literal documented in the config.md table passes', (t) => {
  const r = withFixture(t, {
    rs: 'fn f() { let _ = std::env::var("PICASU_A").ok(); }\n',
    config: tableHeader + tableRow('PICASU_A'),
  })
  assert.equal(r.code, 0, r.out)
})

test('a mention in a message does not count as a read', (t) => {
  const r = withFixture(t, {
    rs: 'fn f() { println!("set PICASU_A in the config file"); }\n',
    config: tableHeader + tableRow('PICASU_A'),
  })
  assert.equal(r.code, 1, r.out)
  assert.match(r.out, /PICASU_A/)
})

test('set_var writes do not count as reads', (t) => {
  const r = withFixture(t, {
    rs: 'fn f() { std::env::set_var("PICASU_A", "true"); }\n',
    config: tableHeader + tableRow('PICASU_A'),
  })
  assert.equal(r.code, 1, r.out)
  assert.match(r.out, /PICASU_A/)
})

test('commented-out lookups do not count as reads', (t) => {
  const r = withFixture(t, {
    rs: '// std::env::var("PICASU_A")\n/// doc mentioning "PICASU_A"\nfn f() {}\n',
    config: tableHeader + tableRow('PICASU_A'),
  })
  assert.equal(r.code, 1, r.out)
  assert.match(r.out, /PICASU_A/)
})

test('README alone does not satisfy documentation', (t) => {
  const r = withFixture(t, {
    rs: 'fn f() { let _ = std::env::var("PICASU_A").ok(); }\n',
    config: tableHeader,
    readme: '| `PICASU_A` | documented only here |\n',
  })
  assert.equal(r.code, 1, r.out)
  assert.match(r.out, /PICASU_A/)
})

test('config.md prose alone does not satisfy documentation', (t) => {
  const r = withFixture(t, {
    rs: 'fn f() { let _ = std::env::var("PICASU_A").ok(); }\n',
    config: tableHeader + '\nSee `PICASU_A` for details.\n',
  })
  assert.equal(r.code, 1, r.out)
  assert.match(r.out, /PICASU_A/)
})

test('a constructed name fails as unverifiable', (t) => {
  const r = withFixture(t, {
    rs: 'fn f(k: &str) { let _ = std::env::var(&format!("PICASU_{}", k)).ok(); }\n',
    config: tableHeader + tableRow('PICASU_A'),
  })
  assert.equal(r.code, 1, r.out)
  assert.match(r.out, /unverifiable/i)
})

test('const definition plus env::var use counts as a read', (t) => {
  const r = withFixture(t, {
    rs: 'const PICASU_A_ENV: &str = "PICASU_A";\nfn f() { let _ = std::env::var(PICASU_A_ENV).ok(); }\n',
    config: tableHeader + tableRow('PICASU_A'),
  })
  assert.equal(r.code, 0, r.out)
})

test('a const nobody passes to env::var is not a read', (t) => {
  const r = withFixture(t, {
    rs: 'const PICASU_A_ENV: &str = "PICASU_A";\n',
    config: tableHeader + tableRow('PICASU_A'),
  })
  assert.equal(r.code, 1, r.out)
  assert.match(r.out, /PICASU_A/)
})

test('config-field env_var entries count as reads', (t) => {
  const r = withFixture(t, {
    rs: 'struct F { env_var: Option<&\'static str> }\nconst FIELDS: [F; 1] = [F { env_var: Some("PICASU_A") }];\n',
    config: tableHeader + tableRow('PICASU_A'),
  })
  assert.equal(r.code, 0, r.out)
})

test('resolve_root helper arguments count as reads', (t) => {
  const r = withFixture(t, {
    rs: 'fn resolve_root(name: &str) { let _ = std::env::var(name); }\nfn f() { resolve_root("PICASU_A"); }\n',
    config: tableHeader + tableRow('PICASU_A'),
  })
  assert.equal(r.code, 0, r.out)
})

test('a token only README lists is reported as stale README content', (t) => {
  const r = withFixture(t, {
    rs: 'fn f() { let _ = std::env::var("PICASU_A").ok(); }\n',
    config: tableHeader + tableRow('PICASU_A'),
    readme: '| `PICASU_A` | ok |\n| `PICASU_STALE` | gone from config.md |\n',
  })
  assert.equal(r.code, 1, r.out)
  assert.match(r.out, /PICASU_STALE/)
  assert.match(r.out, /README/)
})

test('the PICASU_LOGIN_ prefix row is not a variable name', (t) => {
  const r = withFixture(t, {
    rs: '',
    config: tableHeader + '| `PICASU_LOGIN_*` | family of knobs |\n',
  })
  assert.equal(r.code, 0, r.out)
})
