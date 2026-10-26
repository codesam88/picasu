#!/usr/bin/env node
// Check that the PICASU_* env vars the backend actually reads match the
// variables listed in the docs/config.md table.
//
// A read is one of the code's env-read conduits — plain-string matching
// would count mentions in messages, comments or set_var writes as reads
// and mask documented-but-unread drift:
//   1. env::var / env::var_os called with a string literal;
//   2. a const/static literal whose name is passed to env::var (both the
//      definition and the use must exist);
//   3. `env_var: Some("...")` config-field entries — the override table
//      fed to apply_env_overrides_with, which calls env::var per name;
//   4. resolve_root / resolve_root_path("...") helpers — they read the
//      name internally via env::var.
// Unknown conduits surface as documented-but-unread failures, which is
// the safe direction: extend this list (with a test) when one appears.
//
// Comments are stripped before matching. Dynamic lookups: bare
// identifiers are resolved against the const table or reported as notes;
// constructed names (format!/concat!/anything not a literal or bare
// ident) fail as unverifiable — a PICASU_* name that cannot be checked
// must not slip through.
//
// Documented = PICASU_* tokens on table rows of docs/config.md. README is
// informal: it can neither satisfy documentation nor list a token the
// config.md table lacks. The bare prefix token `PICASU_LOGIN_` (prose
// about the PICASU_LOGIN_* family) is not a variable name.
//
// Excluded from the read set (test/dev-only knobs, not supported server
// configuration): PICASU_RANDOM_SEEDS (randomized test scenario seeds),
// PICASU_STORE_TEST_CHILD / _PATH / _READY (cross-process user-store tests).
//
// Failures (exit 1): read not documented, documented with no read, README
// token missing from the config.md table, unverifiable dynamic lookup.
//
// Usage: env-var-lint.mjs [repo-root]
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const root = process.argv[2] ?? '.'
const backendSrc = join(root, 'backend', 'src')
const docTablePath = join(root, 'docs', 'config.md')
const readmePath = join(root, 'README.md')

const EXCLUDED = new Set([
  'PICASU_RANDOM_SEEDS',
  'PICASU_STORE_TEST_CHILD',
  'PICASU_STORE_TEST_PATH',
  'PICASU_STORE_TEST_READY',
])

const TOKEN_RE = /PICASU_[A-Z0-9_]+/g
const PREFIX_TOKEN = 'PICASU_LOGIN_'

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    const st = statSync(full)
    if (st.isDirectory()) {
      walk(full, out)
    } else if (full.endsWith('.rs')) {
      out.push(full)
    }
  }
  return out
}

// Remove // and /* */ comments, string-literal aware (a `//` inside a
// string is content, not a comment). Raw strings are not handled; a
// misparse here only ever removes real code, which fails closed as
// documented-but-unread.
function stripComments(text) {
  let out = ''
  let i = 0
  let blockDepth = 0
  const n = text.length
  while (i < n) {
    const c = text[i]
    if (blockDepth > 0) {
      if (text.startsWith('/*', i)) { blockDepth += 1; i += 2; continue }
      if (text.startsWith('*/', i)) { blockDepth -= 1; i += 2; continue }
      i += 1
      continue
    }
    if (c === '"') {
      out += c
      i += 1
      while (i < n) {
        if (text[i] === '\\') {
          out += text[i] + (text[i + 1] ?? '')
          i += 2
          continue
        }
        out += text[i]
        const closed = text[i] === '"'
        i += 1
        if (closed) break
      }
      continue
    }
    if (text.startsWith('//', i)) {
      while (i < n && text[i] !== '\n') i += 1
      continue
    }
    if (text.startsWith('/*', i)) {
      blockDepth = 1
      i += 2
      continue
    }
    out += c
    i += 1
  }
  return out
}

function lineOf(text, index) {
  return text.slice(0, index).split('\n').length
}

const readLiterals = new Set() // PICASU_* literals in a read conduit
const constDefs = new Map() // const/static NAME -> string value
const bareIdents = new Map() // identifier passed to env::var -> [sites]
const unverifiable = [] // constructed env::var arguments -> FAIL

const ENV_CALL_RE = /\b(?:std::)?env::var(?:_os)?\s*\(/g
const CONST_DEF_RE = /\b(?:const|static)\s+([A-Za-z_]\w*)\s*:[^=\n]*=\s*"([^"]*)"/g
const FIELD_RE = /\benv_var\s*:\s*(?:Some\(\s*)?"([^"]*)"/g
const ROOT_RE = /\bresolve_root(?:_path)?\s*\(\s*"([^"]*)"/g

function collectLiteral(match) {
  const token = match[1]
  if (/^PICASU_/.test(token)) readLiterals.add(token)
}

for (const file of walk(backendSrc)) {
  const raw = readFileSync(file, 'utf8')
  const text = stripComments(raw)
  const shown = relative(root, file)

  // Conduit 1: env::var/literal, and classification of every dynamic form.
  ENV_CALL_RE.lastIndex = 0
  for (let m = ENV_CALL_RE.exec(text); m !== null; m = ENV_CALL_RE.exec(text)) {
    const open = m.index + m[0].length - 1
    let depth = 0
    let close = -1
    for (let j = open; j < text.length; j += 1) {
      if (text[j] === '(') depth += 1
      else if (text[j] === ')') {
        depth -= 1
        if (depth === 0) { close = j; break }
      }
    }
    if (close === -1) {
      unverifiable.push(`${shown}:${lineOf(text, m.index)}: unterminated env::var argument`)
      continue
    }
    const arg = text.slice(open + 1, close).trim()
    const line = `${shown}:${lineOf(text, m.index)}`
    if (arg.startsWith('"')) {
      const lit = /^"((?:[^"\\]|\\.)*)"/.exec(arg)
      if (lit && /^PICASU_/.test(lit[1])) readLiterals.add(lit[1])
      continue
    }
    const ident = /^&?\s*([A-Za-z_]\w*)$/.exec(arg)
    if (ident) {
      const sites = bareIdents.get(ident[1]) ?? []
      sites.push(line)
      bareIdents.set(ident[1], sites)
      continue
    }
    unverifiable.push(`${line}: env::var(${arg.slice(0, 60)})`)
  }

  // Conduits 3 and 4: config-field entries and the resolve_root helpers.
  for (const m of text.matchAll(FIELD_RE)) collectLiteral(m)
  for (const m of text.matchAll(ROOT_RE)) collectLiteral(m)

  for (const m of text.matchAll(CONST_DEF_RE)) {
    constDefs.set(m[1], m[2])
  }
}

// Conduit 2: const/static literal whose name reaches an env::var call.
const dynamicNotes = []
for (const [name, sites] of bareIdents) {
  const value = constDefs.get(name)
  if (value !== undefined && /^PICASU_/.test(value) && !EXCLUDED.has(value)) {
    readLiterals.add(value)
    continue
  }
  const suffix = value !== undefined ? ' (excluded test/dev knob)' : ' (supplied by caller/table)'
  for (const site of sites) dynamicNotes.push(`${site}: ${name}${suffix}`)
}

const codeUniverse = new Set([...readLiterals].filter((t) => !EXCLUDED.has(t)))

// Documented: config.md table rows; README tokens are informational only.
function tokensOf(path, { tableRowsOnly }) {
  let text
  try {
    text = readFileSync(path, 'utf8')
  } catch (error) {
    console.error(`${relative(root, path)}: cannot read — ${error.message}`)
    process.exit(1)
  }
  const tokens = new Set()
  for (const line of text.split('\n')) {
    if (tableRowsOnly && !line.trim().startsWith('|')) continue
    for (const m of line.matchAll(TOKEN_RE)) {
      if (m[0] === PREFIX_TOKEN) continue
      tokens.add(m[0])
    }
  }
  return tokens
}
const docTable = tokensOf(docTablePath, { tableRowsOnly: true })
const readmeTokens = tokensOf(readmePath, { tableRowsOnly: false })

// --- compare ---
const undocumented = [...codeUniverse].filter((t) => !docTable.has(t)).sort()
const unread = [...docTable].filter((t) => !codeUniverse.has(t)).sort()
const staleReadme = [...readmeTokens].filter((t) => !docTable.has(t)).sort()

let failed = false
if (undocumented.length) {
  failed = true
  console.error('env-var-check: FAIL — PICASU_* env vars read by backend but not in the docs/config.md table:')
  for (const t of undocumented) console.error(`  - ${t}`)
}
if (unread.length) {
  failed = true
  console.error('env-var-check: FAIL — docs/config.md table vars with no read conduit in backend/src:')
  for (const t of unread) console.error(`  - ${t}`)
}
if (staleReadme.length) {
  failed = true
  console.error('env-var-check: FAIL — README mentions vars the docs/config.md table lacks:')
  for (const t of staleReadme) console.error(`  - ${t} (README is informal; document it in docs/config.md)`)
}
if (unverifiable.length) {
  failed = true
  console.error('env-var-check: FAIL — unverifiable dynamic env::var argument (use a string literal or a const):')
  for (const s of unverifiable) console.error(`  - ${s}`)
}

if (dynamicNotes.length) {
  console.error('env-var-check: note — identifier lookups not individually verifiable:')
  for (const s of dynamicNotes) console.error(`  - ${s}`)
}

if (failed) process.exit(1)

console.log(
  `env-var-check: OK — ${codeUniverse.size} read vars == ${docTable.size} documented table vars`
)
process.exit(0)
