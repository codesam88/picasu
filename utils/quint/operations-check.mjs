#!/usr/bin/env node
// Validate utils/quint/pipeline/operations.json against events.json, and check
// that every tracked operation is anchored by at least one
// #[quint::operation(...)] marker on a real item in backend/src/**/*.rs.
//
// Rung 1 upgrades over rung 0:
//   - markers are only counted when they appear as an attribute on a real
//     item (a line matching `^\s*#\[[...]`); comment, doc-comment, string or
//     `#[cfg(test)]`-gated mentions do not count;
//   - every event in events.json must be owned by at least one operation
//     (it appears in that operation's `events` array) or carry an explicit
//     `no_owner_reason`;
//   - model `@operation` tags in asset-aspects.qnt must name known OPs, and
//     guarded OPs without a model tag are reported (not failing).
//
// Usage: operations-check.mjs [root-or-pipeline-dir]
import { readFileSync, existsSync, readdirSync, statSync } from 'node:fs'
import { join, resolve, relative } from 'node:path'

function resolveBase(input) {
  const p = resolve(input ?? '.')
  if (existsSync(join(p, 'operations.json'))) return p
  if (existsSync(join(p, 'utils', 'quint', 'pipeline', 'operations.json')))
    return join(p, 'utils', 'quint', 'pipeline')
  return p
}

const base = resolveBase(process.argv[2] ?? '.')
const opsPath = join(base, 'operations.json')
const eventsPath = join(base, 'events.json')
// base is pipeline dir when invoked with repo root resolution below; handle both.
const repoRoot = base.endsWith('pipeline') ? resolve(base, '..', '..', '..') : base

let failed = false
function fail(msg) { console.error(`FAIL: ${msg}`); failed = true }
function warn(msg) { console.warn(`WARN: ${msg}`) }

function load(path) {
  try { return JSON.parse(readFileSync(path, 'utf8')) } catch (e) {
    console.error(`${path}: ${e.message}`)
    process.exit(1)
  }
}

const opsDoc = load(opsPath)
const eventsDoc = existsSync(eventsPath) ? load(eventsPath) : null

// Top-level sanity.
if (typeof opsDoc.version !== 'number') fail('operations.json: missing/invalid version')
if (typeof opsDoc.description !== 'string' || opsDoc.description.trim() === '') {
  fail('operations.json: missing/empty top-level description')
}

const ASPECTS = new Set(['asset_file', 'sidecar', 'thumbnail', 'db_row'])
const STATUSES = new Set(['guarded', 'open', 'deferred'])
const NODES = new Set(Array.from({ length: 13 }, (_, i) => `N${i + 1}`))
const EXPECTED_OPS = new Set(Array.from({ length: 12 }, (_, i) => `OP-${i + 1}`))

const eventNames = new Set(
  Array.isArray(eventsDoc?.events) ? eventsDoc.events.map((e) => e.name) : [],
)

const entries = Array.isArray(opsDoc.operations) ? opsDoc.operations : []
if (!Array.isArray(opsDoc.operations)) fail('operations.json: operations must be an array')

const seenIds = new Set()

// Validate operations.json entries.
for (let i = 0; i < entries.length; i++) {
  const op = entries[i]
  const label = `operations[${i}]${op && op.id ? ` (${op.id})` : ''}`
  if (op == null || typeof op !== 'object') { fail(`${label}: not an object`); continue }

  for (const f of ['id', 'name', 'events', 'reads', 'writes', 'guards', 'status', 'nodes', 'notes']) {
    if (!(f in op)) fail(`${label}: missing required field '${f}'`)
  }

  if (!EXPECTED_OPS.has(op.id)) fail(`${label}: id '${op.id}' not in OP-1..OP-12`)
  if (seenIds.has(op.id)) fail(`${label}: duplicate id '${op.id}'`)
  seenIds.add(op.id)

  if (typeof op.name !== 'string' || op.name.trim() === '') fail(`${label}: name missing/empty`)

  if (!Array.isArray(op.events)) fail(`${label}: events must be an array`)
  else for (const e of op.events) {
    if (typeof e !== 'string' || e.trim() === '') fail(`${label}: event entry must be non-empty string`)
    else if (eventNames.size > 0 && !eventNames.has(e)) fail(`${label}: event '${e}' not in events.json`)
  }

  for (const field of ['reads', 'writes']) {
    if (!Array.isArray(op[field])) fail(`${label}: ${field} must be an array`)
    else for (const a of op[field]) {
      if (!ASPECTS.has(a)) fail(`${label}: ${field} item '${a}' not in aspect set`)
    }
  }

  if (!Array.isArray(op.guards)) fail(`${label}: guards must be an array`)
  else for (const g of op.guards) {
    if (typeof g !== 'string') fail(`${label}: guard must be string`)
  }

  if (!STATUSES.has(op.status)) fail(`${label}: status '${op.status}' not in guarded/open/deferred`)

  if (!Array.isArray(op.nodes)) fail(`${label}: nodes must be an array`)
  else for (const n of op.nodes) {
    if (!NODES.has(n)) fail(`${label}: node '${n}' not in N1..N13`)
  }

  if (typeof op.notes !== 'string') fail(`${label}: notes must be a string`)
}

for (const id of EXPECTED_OPS) {
  if (!seenIds.has(id)) fail(`missing operation '${id}' in operations.json`)
}

// Scan backend/src/**/*.rs for markers, counting only real attribute lines.
const backendSrc = join(repoRoot, 'backend', 'src')
function walkRs(dir, out) {
  if (!existsSync(dir)) return
  for (const name of readdirSync(dir)) {
    const full = join(dir, name)
    let st
    try { st = statSync(full) } catch { continue }
    if (st.isDirectory()) walkRs(full, out)
    else if (name.endsWith('.rs')) out.push(full)
  }
}
const rsFiles = []
walkRs(backendSrc, rsFiles)

// Attribute-aware marker collection: a marker only counts if it sits on a
// line that is itself an attribute (`^\s*#\[[...]`), and its attribute
// block is not gated by `#[cfg(test)]` / `#[test]`.
function collectFileMarkers(text) {
  const lines = text.split('\n')
  const opMarkers = []
  const eventMarkers = []
  let block = []
  for (const line of lines) {
    if (/^\s*#\[/.test(line)) {
      block.push(line)
      const opm = line.match(/quint::operation\(\s*"([^"]+)"\s*\)/)
      if (opm) {
        const testGated = block.some((l) => /cfg\s*\(\s*test\s*\)/.test(l) || /^\s*#\[\s*test\s*\]/.test(l))
        if (!testGated) opMarkers.push(opm[1])
      }
      const evm = line.match(/quint::event\(\s*"([^"]+)"\s*\)/)
      if (evm) {
        const testGated = block.some((l) => /cfg\s*\(\s*test\s*\)/.test(l) || /^\s*#\[\s*test\s*\]/.test(l))
        if (!testGated) eventMarkers.push(evm[1])
      }
      continue
    }
    // reset attribute block on any non-attribute line
    block = []
    if (line.trim() === '') continue
  }
  return { opMarkers, eventMarkers }
}

const markerOps = new Set()
const markerOpCounts = new Map()
const knownEvents = eventNames.size > 0 ? eventNames : null

for (const file of rsFiles) {
  const text = readFileSync(file, 'utf8')
  const { opMarkers, eventMarkers } = collectFileMarkers(text)
  for (const id of opMarkers) {
    if (!EXPECTED_OPS.has(id)) fail(`marker in ${relative(repoRoot, file)}: unknown operation '${id}'`)
    else {
      markerOps.add(id)
      markerOpCounts.set(id, (markerOpCounts.get(id) ?? 0) + 1)
    }
  }
  for (const ev of eventMarkers) {
    if (knownEvents && !knownEvents.has(ev)) fail(`marker in ${relative(repoRoot, file)}: unknown event '${ev}'`)
  }
}

// Require every operation to have at least one marker.
for (const id of EXPECTED_OPS) {
  if (!markerOps.has(id)) fail(`operation '${id}' has no #[quint::operation(...)] marker on a real item in backend/src`)
}

// Event ownership: every event owned by >=1 operation, or explicitly exempt
// with a recorded reason (rung 1 ownership rule). Exempt events are read-only
// boundary events (API_READ_*) and recovery events (PROCESS_CRASH,
// DERIVED_STORE_LOSS, DB_LOSS) that are not tied to a single tracked
// mutation operation.
const EVENT_OWNER_EXEMPT = new Map([
  ['API_READ_COMPRESSED', 'read-only boundary event: serves compressed bytes; no mutation operation tracked'],
  ['API_READ_ORIGINAL', 'read-only boundary event: serves original bytes; no mutation operation tracked'],
  ['API_READ_MISSING', 'read-only boundary event: missing-asset response path; no mutation operation tracked'],
  ['API_READ_MOVED', 'read-only boundary event: moved-asset response path; no mutation operation tracked'],
  ['PROCESS_CRASH', 'recovery event: crash recovery is handled by the pipeline generally'],
  ['DERIVED_STORE_LOSS', 'recovery event: derived-store loss recovery is handled by the pipeline generally'],
  ['DB_LOSS', 'recovery event: DB loss recovery is handled by the pipeline generally'],
])
const ownedEvents = new Set()
for (const op of entries) {
  for (const e of (op.events ?? [])) ownedEvents.add(e)
}
if (eventsDoc && Array.isArray(eventsDoc.events)) {
  const seenEventNames = new Set(eventsDoc.events.map((e) => e?.name).filter(Boolean))
  for (const name of seenEventNames) {
    const owned = ownedEvents.has(name)
    if (!owned && !EVENT_OWNER_EXEMPT.has(name)) {
      fail(`event '${name}' has no owning operation and is not in the exempt set`)
    }
  }
  // Exempt set must not reference unknown events.
  for (const name of EVENT_OWNER_EXEMPT.keys()) {
    if (!seenEventNames.has(name)) fail(`exempt event '${name}' not present in events.json`)
  }
  // Exempt events must not actually be owned (would make the exemption stale).
  for (const name of EVENT_OWNER_EXEMPT.keys()) {
    if (ownedEvents.has(name)) warn(`event '${name}' is exempt but also owned by an operation (stale exemption)`)
  }
}

// Validate @operation OP-N tags in asset-aspects.qnt.
const qntPath = join(repoRoot, 'utils', 'quint', 'pipeline', 'asset-aspects.qnt')
const modelTaggedOps = new Set()
if (existsSync(qntPath)) {
  const text = readFileSync(qntPath, 'utf8')
  const re = /@operation\s+(OP-\d+)/g
  let m
  while ((m = re.exec(text)) !== null) {
    const id = m[1]
    if (!EXPECTED_OPS.has(id)) fail(`@operation tag in asset-aspects.qnt: unknown '${id}'`)
    else modelTaggedOps.add(id)
  }
}

// Report-only: guarded OPs without a model @operation tag.
const guardedOps = entries.filter((o) => o?.status === 'guarded').map((o) => o.id)
const guardedUntagged = guardedOps.filter((id) => !modelTaggedOps.has(id))
if (guardedUntagged.length > 0) {
  warn(`report-only: guarded OPs without a model @operation tag: ${guardedUntagged.join(', ')}`)
} else {
  console.log('report: all guarded OPs have a model @operation tag')
}

if (failed) {
  console.error('operations-check: FAILED')
  process.exit(1)
}

const countSummary = [...EXPECTED_OPS].map((id) => `${id}=${markerOpCounts.get(id) ?? 0}`).join(' ')
console.log(`operations-check: OK (${seenIds.size} operations, ${markerOps.size} marked, ${rsFiles.length} rs files scanned)`)
console.log(`  marker counts: ${countSummary}`)
console.log(`  model tags: ${modelTaggedOps.size} OPs tagged; guarded untagged: ${guardedUntagged.length}`)
process.exit(0)