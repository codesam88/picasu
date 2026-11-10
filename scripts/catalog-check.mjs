#!/usr/bin/env node
// Validate specs/goals.json, specs/requirements.json,
// specs/assumptions.json and specs/scope.json against
// specs/catalog.schema.json (manual field walk, same approach as
// events-check.mjs), plus the cross-catalog references: goals ->
// assumptions ids, requirements -> committed goals, and every committed
// goal must derive at least one requirement.
//
// Usage: catalog-check.mjs [root-or-specs-dir]
import { readFileSync, existsSync } from 'node:fs'
import { join, resolve } from 'node:path'

const arg = process.argv[2] ?? '.'
const base = resolveBase(arg)

function resolveBase(input) {
  const p = resolve(input)
  if (existsSync(join(p, 'goals.json'))) return p
  if (existsSync(join(p, 'specs', 'goals.json'))) return join(p, 'specs')
  return p
}

let failed = false
function fail(msg) { console.error(`FAIL: ${msg}`); failed = true }

function load(name) {
  try { return JSON.parse(readFileSync(join(base, name), 'utf8')) } catch (e) {
    console.error(`${name}: ${e.message}`)
    process.exit(1)
  }
}

const goalsDoc = load('goals.json')
const assumptionsDoc = load('assumptions.json')
const requirementsDoc = load('requirements.json')
const scopeDoc = load('scope.json')

const catalogSchema = load('catalog.schema.json')

const goalIds = catalogSchema.definitions.goal.properties.id.enum
const decisions = catalogSchema.definitions.goal.properties.decision.enum
const anchorKinds = catalogSchema.definitions.anchor.properties.kind.enum
const assumptionIdsEnum = catalogSchema.definitions.assumption.properties.id.enum
const statuses = catalogSchema.definitions.assumption.properties.status.enum
const requirementIdsEnum = catalogSchema.definitions.requirement.properties.id.enum
const requirementStatuses = catalogSchema.definitions.requirement.properties.status.enum
const scopeIdsEnum = catalogSchema.definitions.scope.properties.id.enum

function nonEmptyString(s) { return typeof s === 'string' && s.trim() !== '' }

// assumptions first: goals cross-reference their ids
const assumptionList = Array.isArray(assumptionsDoc.assumptions) ? assumptionsDoc.assumptions : []
const assumptionIds = new Set()
for (let i = 0; i < assumptionList.length; i++) {
  const a = assumptionList[i]
  const label = `assumptions[${i}]${a && a.id ? ` (${a.id})` : ''}`
  if (a == null || typeof a !== 'object') { fail(`${label}: not an object`); continue }
  if (!assumptionIdsEnum.includes(a.id)) fail(`${label}: id '${a.id}' not in schema enum`)
  if (assumptionIds.has(a.id)) fail(`${label}: duplicate id`)
  assumptionIds.add(a.id)
  for (const f of ['title', 'owner']) {
    if (!nonEmptyString(a[f])) fail(`${a.id}: missing/empty '${f}'`)
  }
  if (!statuses.includes(a.status)) {
    fail(`${a.id}: status '${a.status}' not in ${statuses.join('/')}`)
  }
}

const scopeList = Array.isArray(scopeDoc.scopes) ? scopeDoc.scopes : []
const scopeIds = new Set()
for (let i = 0; i < scopeList.length; i++) {
  const s = scopeList[i]
  const label = `scopes[${i}]${s && s.id ? ` (${s.id})` : ''}`
  if (s == null || typeof s !== 'object') { fail(`${label}: not an object`); continue }
  if (!scopeIdsEnum.includes(s.id)) fail(`${label}: id '${s.id}' not in schema enum`)
  if (scopeIds.has(s.id)) fail(`${label}: duplicate id`)
  scopeIds.add(s.id)
  for (const f of ['title', 'bound', 'why_safe', 'lifting']) {
    if (!nonEmptyString(s[f])) fail(`${s.id}: missing/empty '${f}'`)
  }
  if ('provisional' in s && typeof s.provisional !== 'boolean') {
    fail(`${s.id}: provisional must be a boolean when present`)
  }
}

const goalList = Array.isArray(goalsDoc.goals) ? goalsDoc.goals : []
const seenGoals = new Set()
for (let i = 0; i < goalList.length; i++) {
  const g = goalList[i]
  const label = `goals[${i}]${g && g.id ? ` (${g.id})` : ''}`
  if (g == null || typeof g !== 'object') { fail(`${label}: not an object`); continue }
  if (!goalIds.includes(g.id)) fail(`${label}: id '${g.id}' not in schema enum`)
  if (seenGoals.has(g.id)) fail(`${label}: duplicate id`)
  seenGoals.add(g.id)
  if (!decisions.includes(g.decision)) {
    fail(`${g.id}: decision '${g.decision}' not in ${decisions.join('/')}`)
  }
  for (const f of ['title', 'description']) {
    if (!nonEmptyString(g[f])) fail(`${g.id}: missing/empty '${f}'`)
  }
  if (!Array.isArray(g.anchors)) fail(`${g.id}: anchors must be an array`)
  else {
    if (g.decision === 'committed' && g.anchors.length === 0) {
      fail(`${g.id}: committed goal has no anchor (predicate, code or test)`)
    }
    for (let n = 0; n < g.anchors.length; n++) {
      const an = g.anchors[n]
      if (an == null || !anchorKinds.includes(an.kind)) {
        fail(`${g.id}.anchors[${n}]: kind '${an?.kind}' not in ${anchorKinds.join('/')}`)
      }
      if (!nonEmptyString(an?.ref)) fail(`${g.id}.anchors[${n}]: missing/empty ref`)
    }
  }
  for (const ref of g.assumptions ?? []) {
    if (!assumptionIds.has(ref)) fail(`${g.id}: assumption ref '${ref}' not in assumptions.json`)
  }
}

const committedGoals = new Set(
  goalList.filter(g => g && g.id && g.decision === 'committed').map(g => g.id)
)
const requirementList = Array.isArray(requirementsDoc.requirements) ? requirementsDoc.requirements : []
const requirementIds = new Set()
for (let i = 0; i < requirementList.length; i++) {
  const r = requirementList[i]
  const label = `requirements[${i}]${r && r.id ? ` (${r.id})` : ''}`
  if (r == null || typeof r !== 'object') { fail(`${label}: not an object`); continue }
  if (!requirementIdsEnum.includes(r.id)) fail(`${label}: id '${r.id}' not in schema enum`)
  if (requirementIds.has(r.id)) fail(`${label}: duplicate id`)
  requirementIds.add(r.id)
  for (const f of ['title', 'statement', 'owner', 'gate']) {
    if (!nonEmptyString(r[f])) fail(`${r.id}: missing/empty '${f}'`)
  }
  if (!Array.isArray(r.derived_from) || r.derived_from.length < 1 ||
      !r.derived_from.every(id => goalIds.includes(id))) {
    fail(`${r.id}: derived_from must be a non-empty array of goal ids`)
  } else {
    for (const gid of r.derived_from) {
      if (!committedGoals.has(gid)) {
        fail(`${r.id}: derives from ${gid}, which is not a committed goal`)
      }
    }
  }
  if (!requirementStatuses.includes(r.status)) {
    fail(`${r.id}: status '${r.status}' not in ${requirementStatuses.join('/')}`)
  }
}
for (const gid of committedGoals) {
  if (!requirementList.some(r => (r.derived_from ?? []).includes(gid))) {
    fail(`committed goal ${gid} derives no requirements`)
  }
}

if (failed) { console.error('catalog-check: FAILED'); process.exit(1) }
console.log(`catalog-check: OK (${goalList.length} goals, ${requirementIds.size} requirements, ${assumptionIds.size} assumptions, ${scopeIds.size} scopes)`)
