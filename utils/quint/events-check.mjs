#!/usr/bin/env node
// Validate utils/quint/pipeline/events.json against utils/quint/pipeline/catalog.schema.json.
// Inputs carry no references into the model that consumes them; the
// event-to-action mapping lives in the model module's header.
//
// Usage: events-check.mjs [root-or-specs-dir] [--strict]
//   root may be a repo root (looks in <root>/specs) or the specs dir itself.
import { readFileSync, existsSync } from 'node:fs'
import { join, resolve } from 'node:path'

const args = process.argv.slice(2)
let strict = false
const positional = []
for (const a of args) {
  if (a === '--strict') strict = true
  else positional.push(a)
}

function resolveBase(input) {
  const p = resolve(input ?? '.')
  if (existsSync(join(p, 'events.json'))) return p
  if (existsSync(join(p, 'utils', 'quint', 'pipeline', 'events.json'))) return join(p, 'utils', 'quint', 'pipeline')
  return p
}

const base = resolveBase(positional[0] ?? '.')
const eventsPath = join(base, 'events.json')
const schemaPath = join(base, 'catalog.schema.json')

let failed = false
function fail(msg) { console.error(`FAIL: ${msg}`); failed = true }
function warn(msg) { console.warn(`WARN: ${msg}`) }

// Load JSON files.
let eventsDoc, schema
try { eventsDoc = JSON.parse(readFileSync(eventsPath, 'utf8')) } catch (e) { console.error(`${eventsPath}: ${e.message}`); process.exit(1) }
try { schema = JSON.parse(readFileSync(schemaPath, 'utf8')) } catch (e) { console.error(`${schemaPath}: ${e.message}`); process.exit(1) }

const defs = schema.definitions ?? {}
const eventDef = defs.event ?? {}
const eventProps = eventDef.properties ?? {}
const nameEnum = eventProps.name?.enum ?? []
const affectedEnum = eventProps.affected_state?.items?.enum ?? []
const requiredFields = eventDef.required ?? []
const variantRequired = (defs.variant?.required) ?? []

const STATUSES = new Set(['established', 'proposed'])
const FAULT_STATUSES = new Set(['covered', 'open', 'tbd'])
function isTodoCodeRef(ref) { return typeof ref === 'string' && ref.includes('TODO') }

const entries = Array.isArray(eventsDoc.events) ? eventsDoc.events : []
const seenNames = new Set()
const todoRefs = []

// Top-level sanity.
if (typeof eventsDoc.version !== 'number') fail('events.json: missing/invalid version')
if (typeof eventsDoc.description !== 'string' || eventsDoc.description.trim() === '') {
  fail('events.json: missing/empty top-level description')
}

for (let i = 0; i < entries.length; i++) {
  const ev = entries[i]
  const label = `events[${i}]${ev && ev.name ? ` (${ev.name})` : ''}`

  // required fields
  for (const f of requiredFields) {
    if (ev == null || !(f in ev)) fail(`${label}: missing required field '${f}'`)
  }
  if (ev == null) continue

  // name enum
  if (!nameEnum.includes(ev.name)) fail(`${label}: name '${ev.name}' not in schema enum`)
  if (seenNames.has(ev.name)) fail(`${label}: duplicate event name '${ev.name}'`)
  seenNames.add(ev.name)

  // status (established | proposed)
  if (!STATUSES.has(ev.status)) fail(`${label}: status '${ev.status}' not in ${[...STATUSES].join('/')}`)

  // description: self-explanatory event text, never empty
  if (typeof ev.description !== 'string' || ev.description.trim() === '') {
    fail(`${label}: description missing or empty`)
  }

  // effects: settled-state claims, at least one
  if (!Array.isArray(ev.effects) || ev.effects.length < 1 ||
      !ev.effects.every(s => typeof s === 'string' && s.trim() !== '')) {
    fail(`${label}: effects must be a non-empty array of non-empty strings`)
  }

  // faults: error cases with a status
  if (!Array.isArray(ev.faults)) {
    fail(`${label}: faults must be an array`)
  } else for (let f = 0; f < ev.faults.length; f++) {
    const fl = ev.faults[f]
    const flabel = `${label}.faults[${f}]`
    if (fl == null || typeof fl !== 'object' || Array.isArray(fl)) {
      fail(`${flabel}: must be an object`)
      continue
    }
    for (const key of ['fault', 'consequence']) {
      if (typeof fl[key] !== 'string' || fl[key].trim() === '') fail(`${flabel}: missing/empty '${key}'`)
    }
    if (!FAULT_STATUSES.has(fl.status)) fail(`${flabel}: status '${fl.status}' not in ${[...FAULT_STATUSES].join('/')}`)
  }

  // affected_state
  if (!Array.isArray(ev.affected_state)) fail(`${label}: affected_state must be array`)
  else for (const s of ev.affected_state) {
    if (!affectedEnum.includes(s)) fail(`${label}: affected_state item '${s}' not in enum`)
  }

  // code_ref TODO tracking
  if (isTodoCodeRef(ev.code_ref)) todoRefs.push(`${ev.name}: ${ev.code_ref}`)

  // variants
  if (ev.variants != null) {
    if (!Array.isArray(ev.variants)) fail(`${label}: variants must be array`)
    else for (let v = 0; v < ev.variants.length; v++) {
      const vr = ev.variants[v]
      const vlabel = `${label}.variants[${v}]`
      for (const f of variantRequired) {
        if (vr == null || !(f in vr)) fail(`${vlabel}: missing required field '${f}'`)
      }
      if (vr == null) continue
      if (isTodoCodeRef(vr.code_ref)) todoRefs.push(`${ev.name}/${vr.name}: ${vr.code_ref}`)
    }
  }
}

// Check all schema names present (informational but fail if count mismatch)
if (entries.length !== nameEnum.length) {
  fail(`events count ${entries.length} != schema enum count ${nameEnum.length}`)
}
for (const n of nameEnum) {
  if (!seenNames.has(n)) fail(`missing event '${n}'`)
}

// TODO reporting.
if (todoRefs.length > 0) {
  console.warn(`TODO code_refs (${todoRefs.length}):`)
  for (const t of todoRefs) console.warn(`  - ${t}`)
  if (strict) {
    fail(`--strict: ${todoRefs.length} TODO code_ref(s) present`)
  }
}

if (failed) {
  console.error('events-check: FAILED')
  process.exit(1)
}
console.log(`events-check: OK (${entries.length} events, ${todoRefs.length} TODO code_refs)`)
