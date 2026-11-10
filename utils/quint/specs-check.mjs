#!/usr/bin/env node
// specs-check: enforce the modeling pattern over committed files.
// No new behavior without a traceable home, no home without a check.
//
// Usage: specs-check.mjs [repo-root | specs-dir] [--generate]
//   Default mode validates and fails on any violation, including a
//   utils/quint/pipeline/traceability.json map that differs from the generated one.
//   --generate rewrites that map instead of checking it.
//
// Rules enforced:
//   1. Every `action`, `val`, `pure def`, and `run` in utils/quint/pipeline/*.qnt carries
//      at least one machine tag (`// @assumes ASS-N`,
//      `// @requires REQ-N`, `// @scope SCOPE-N`) or an explicit
//      `// @exempt <reason>` with reason in {environment, no-backing, observer}.
//   2. Every tag references a registered ID: ASS from utils/quint/pipeline/assumptions.json,
//      REQ from utils/quint/pipeline/requirements.json, SCOPE from utils/quint/pipeline/scope.json.
//   3. Every ASS and SCOPE ID is referenced at least once. Requirements are
//      not reverse-covered here: they derive from committed goals and are
//      evidenced by the implementation gate recorded in requirements.json.
//   4. Every scope entry carries non-empty bound, why_safe and lifting
//      fields.
//   5. In check mode, the generated utils/quint/pipeline/traceability.json matches.
import { readFileSync, existsSync, readdirSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const args = process.argv.slice(2)
const generate = args.includes('--generate')
const positional = args.filter(a => !a.startsWith('--'))

const specs = resolveBase(positional[0])
function resolveBase(input) {
  const p = resolve(input ?? '.')
  if (existsSync(join(p, 'events.json'))) return p
  if (existsSync(join(p, 'utils', 'quint', 'pipeline', 'events.json'))) return join(p, 'utils', 'quint', 'pipeline')
  return p
}

let failed = false
const warnings = []
function fail(msg) {
  console.error(`FAIL: ${msg}`)
  failed = true
}
function warn(msg) {
  warnings.push(msg)
  console.error(`WARN: ${msg}`)
}

// ---- registries -----------------------------------------------------------
function read(p) {
  try {
    return readFileSync(p, 'utf8')
  } catch {
    fail(`cannot read ${p}`)
    return ''
  }
}

const assumptionsJson = (() => {
  try {
    return JSON.parse(read(join(specs, 'assumptions.json')))
  } catch (e) {
    fail(`cannot parse assumptions.json: ${e.message}`)
    return { assumptions: [] }
  }
})()
const scopeJson = (() => {
  try {
    return JSON.parse(read(join(specs, 'scope.json')))
  } catch (e) {
    fail(`cannot parse scope.json: ${e.message}`)
    return { scopes: [] }
  }
})()
const requirementsJson = (() => {
  try {
    return JSON.parse(read(join(specs, 'requirements.json')))
  } catch (e) {
    fail(`cannot parse requirements.json: ${e.message}`)
    return { requirements: [] }
  }
})()

const assIds = new Set(assumptionsJson.assumptions.map(a => a.id))
const reqIds = new Set(requirementsJson.requirements.map(r => r.id))

const scopeIds = new Map()
for (const s of scopeJson.scopes) {
  for (const field of ['bound', 'why_safe', 'lifting']) {
    if (typeof s[field] !== 'string' || s[field].trim() === '') {
      fail(`scope.json: ${s.id} has no non-empty \`${field}\` field`)
    }
  }
  scopeIds.set(s.id, { title: s.title })
}

// ---- spec files ------------------------------------------------------------
const qntFiles = readdirSync(specs).filter(f => f.endsWith('.qnt')).sort()
if (qntFiles.length === 0) fail(`no .qnt files in ${specs}`)

const EXEMPT_REASONS = new Set(['environment', 'no-backing', 'observer'])
// symbol -> {file, tags: [{kind, id}], exempt: [reasons]}
const defs = []
for (const f of qntFiles) {
  const lines = read(join(specs, f)).split('\n')
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(/^\s*(?:pure\s+)?(action|val|def|run)\s+([A-Za-z_]\w*)/)
    if (!m) continue
    // collect contiguous comment block directly above
    const tags = []
    for (let j = i - 1; j >= 0 && /^\s*\/\//.test(lines[j]); j--) {
      const line = lines[j]
      for (const tm of line.matchAll(/@assumes\s+(ASS-\d+)\b/g)) tags.push({ kind: 'assumes', id: tm[1], line: j + 1 })
      for (const tm of line.matchAll(/@requires\s+(REQ-\d+)\b/g)) tags.push({ kind: 'requires', id: tm[1], line: j + 1 })
      for (const tm of line.matchAll(/@scope\s+(SCOPE-\d+)\b/g)) tags.push({ kind: 'scope', id: tm[1], line: j + 1 })
      for (const tm of line.matchAll(/@exempt\s+([a-z-]+)/g)) tags.push({ kind: 'exempt', id: tm[1], line: j + 1 })
    }
    defs.push({ file: f, kind: m[1] === 'def' ? 'pure def' : m[1], name: m[2], line: i + 1, tags })
  }
}

// ---- checks -----------------------------------------------------------------
const citedAss = new Map() // id -> [file:symbol]
const citedScope = new Map()
function cite(map, id, where) {
  if (!map.has(id)) map.set(id, [])
  map.get(id).push(where)
}

for (const d of defs) {
  const where = `${d.file}:${d.name}`
  if (d.tags.length === 0) {
    fail(`${where}:${d.line}: ${d.kind} has no @assumes/@requires/@scope/@exempt tag`)
    continue
  }
  for (const t of d.tags) {
    if (t.kind === 'assumes') {
      if (!assIds.has(t.id)) fail(`${where}:${t.line}: unknown assumption ${t.id}`);
      else cite(citedAss, t.id, where)
    } else if (t.kind === 'requires') {
      if (!reqIds.has(t.id)) fail(`${where}:${t.line}: unknown requirement ${t.id}`)
    } else if (t.kind === 'scope') {
      if (!scopeIds.has(t.id)) fail(`${where}:${t.line}: unknown scope ${t.id}`);
      else cite(citedScope, t.id, where)
    } else if (t.kind === 'exempt') {
      if (!EXEMPT_REASONS.has(t.id)) fail(`${where}:${t.line}: unknown @exempt reason '${t.id}' (expected one of ${[...EXEMPT_REASONS].join(', ')})`)
    }
  }
}

for (const id of assIds) {
  if (!citedAss.has(id)) fail(`unreferenced assumption ${id}: no .qnt definition carries @assumes ${id}`)
}
for (const [id, meta] of scopeIds) {
  if (!citedScope.has(id)) fail(`unreferenced scope ${id}: no .qnt definition carries @scope ${id}`)
}
const quintSymbols = new Set(defs.map(d => d.name))
const noBacking = defs.filter(d => d.tags.some(t => t.kind === 'exempt' && t.id === 'no-backing'))
if (noBacking.length > 0) warn(`no-backing exemptions: ${noBacking.map(d => `${d.file}:${d.name}`).join(', ')}`)

// ---- generated traceability --------------------------------------------------
// utils/quint/pipeline/traceability.json is the machine-readable map written here. Rows
// carry title plus citing refs.
const rows = []
function pushRow(id, kind, title, refs) {
  rows.push({ id, kind, title, refs: refs.slice().sort() })
}
for (const a of assumptionsJson.assumptions) {
  pushRow(a.id, 'ASS', a.title, citedAss.get(a.id) ?? [])
}
for (const s of scopeJson.scopes) {
  pushRow(s.id, 'SCOPE', s.title, citedScope.get(s.id) ?? [])
}

const generatedTrace = JSON.stringify({ version: 1, rows }, null, 2) + '\n'
const tracePath = join(specs, 'traceability.json')
const traceText = (() => {
  try {
    return readFileSync(tracePath, 'utf8')
  } catch {
    // Missing on first run is expected in --generate mode.
    if (!process.argv.includes('--generate')) {
      fail(`cannot read ${tracePath}`)
    }
    return ''
  }
})()
{
  const wantGenerate = process.argv.includes('--generate')
  if (wantGenerate) {
    if (traceText !== generatedTrace) {
      writeFileSync(tracePath, generatedTrace)
      console.error('specs-check: regenerated utils/quint/pipeline/traceability.json')
    }
  } else if (traceText !== generatedTrace) {
    fail('utils/quint/pipeline/traceability.json is stale (run `just spec-check --generate` after reviewing the diff)')
  }
}

// ---- summary ------------------------------------------------------------------
if (!failed) {
  const nTag = defs.reduce((a, d) => a + d.tags.filter(t => t.kind !== 'exempt').length, 0)
  const nEx = defs.reduce((a, d) => a + d.tags.filter(t => t.kind === 'exempt').length, 0)
  console.error(`specs-check: OK (${defs.length} definitions, ${nTag} tags, ${nEx} exemptions)`)
  if (warnings.length > 0) console.error(`specs-check: ${warnings.length} warning(s)`)
}
process.exit(failed ? 1 : 0)
