#!/usr/bin/env node
// Validate utils/quint/pipeline/operations.json against events.json, and check
// that every tracked operation is anchored by at least one
// #[quint::operation(...)] marker in backend/src/**/*.rs.
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

let failed = false
function fail(msg) { console.error(`FAIL: ${msg}`); failed = true }

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
const markerOps = new Set()

// Validate operations.json entries.
for (let i = 0; i < entries.length; i++) {
  const op = entries[i]
  const label = `operations[${i}]${op && op.id ? ` (${op.id})` : ''}`
  if (op == null || typeof op !== 'object') { fail(`${label}: not an object`); continue }

  // required fields
  for (const f of ['id', 'name', 'events', 'reads', 'writes', 'guards', 'status', 'nodes', 'notes']) {
    if (!(f in op)) fail(`${label}: missing required field '${f}'`)
  }

  // id
  if (!EXPECTED_OPS.has(op.id)) fail(`${label}: id '${op.id}' not in OP-1..OP-12`)
  if (seenIds.has(op.id)) fail(`${label}: duplicate id '${op.id}'`)
  seenIds.add(op.id)

  // name
  if (typeof op.name !== 'string' || op.name.trim() === '') fail(`${label}: name missing/empty`)

  // events
  if (!Array.isArray(op.events)) fail(`${label}: events must be an array`)
  else for (const e of op.events) {
    if (typeof e !== 'string' || e.trim() === '') fail(`${label}: event entry must be non-empty string`)
    else if (eventNames.size > 0 && !eventNames.has(e)) fail(`${label}: event '${e}' not in events.json`)
  }

  // reads / writes
  for (const field of ['reads', 'writes']) {
    if (!Array.isArray(op[field])) fail(`${label}: ${field} must be an array`)
    else for (const a of op[field]) {
      if (!ASPECTS.has(a)) fail(`${label}: ${field} item '${a}' not in aspect set`)
    }
  }

  // guards
  if (!Array.isArray(op.guards)) fail(`${label}: guards must be an array`)
  else for (const g of op.guards) {
    if (typeof g !== 'string') fail(`${label}: guard must be string`)
  }

  // status
  if (!STATUSES.has(op.status)) fail(`${label}: status '${op.status}' not in guarded/open/deferred`)

  // nodes
  if (!Array.isArray(op.nodes)) fail(`${label}: nodes must be an array`)
  else for (const n of op.nodes) {
    if (!NODES.has(n)) fail(`${label}: node '${n}' not in N1..N13`)
  }

  // notes
  if (typeof op.notes !== 'string') fail(`${label}: notes must be a string`)
}

// Ensure all OP-1..OP-12 present.
for (const id of EXPECTED_OPS) {
  if (!seenIds.has(id)) fail(`missing operation '${id}' in operations.json`)
}

// Resolve repo root from base (pipeline dir).
const repoRoot = base.endsWith('pipeline')
  ? resolve(base, '..', '..', '..')
  : base
const backendSrc = join(repoRoot, 'backend', 'src')

// Scan backend/src/**/*.rs for markers.
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

const opMarkerRe = /quint::operation\(\s*"([^"]+)"\s*\)/g
const eventMarkerRe = /quint::event\(\s*"([^"]+)"\s*\)/g

const knownEvents = eventNames.size > 0 ? eventNames : null
// Also accept any event name from events.json; if events missing, accept any non-empty.

for (const file of rsFiles) {
  const text = readFileSync(file, 'utf8')
  let m
  opMarkerRe.lastIndex = 0
  while ((m = opMarkerRe.exec(text)) !== null) {
    const id = m[1]
    if (!EXPECTED_OPS.has(id)) fail(`marker in ${relative(repoRoot, file)}: unknown operation '${id}'`)
    else markerOps.add(id)
  }
  eventMarkerRe.lastIndex = 0
  while ((m = eventMarkerRe.exec(text)) !== null) {
    const ev = m[1]
    if (knownEvents && !knownEvents.has(ev)) fail(`marker in ${relative(repoRoot, file)}: unknown event '${ev}'`)
  }
}

// Require every operation to have at least one marker.
for (const id of EXPECTED_OPS) {
  if (!markerOps.has(id)) fail(`operation '${id}' has no #[quint::operation(...)] marker in backend/src`)
}

// Validate @operation OP-N tags in asset-aspects.qnt if present.
const qntPath = join(repoRoot, 'utils', 'quint', 'pipeline', 'asset-aspects.qnt')
if (existsSync(qntPath)) {
  const text = readFileSync(qntPath, 'utf8')
  const re = /@operation\s+(OP-\d+)/g
  let m
  while ((m = re.exec(text)) !== null) {
    const id = m[1]
    if (!EXPECTED_OPS.has(id)) fail(`@operation tag in asset-aspects.qnt: unknown '${id}'`)
  }
}

if (failed) {
  console.error('operations-check: FAILED')
  process.exit(1)
}
console.log(`operations-check: OK (${seenIds.size} operations, ${markerOps.size} marked, ${rsFiles.length} rs files scanned)`)
process.exit(0)
