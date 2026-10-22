#!/usr/bin/env node
// Validate the .plan task frontmatter against .plan/.schema.json.
//
// The plan board tool (tablethat) validates the same schema, but it is a
// source-built Rust dependency: every check job had to compile it before it
// could lint. This script is the gate's replacement — the schema file stays
// the single source of truth, and the tool is only needed for the board
// view (`just plan`).
//
// Usage: plan-lint.mjs [repo-root]
import { readFileSync, readdirSync } from 'node:fs'
import { join, relative } from 'node:path'

const root = process.argv[2] ?? '.'
const planDir = join(root, '.plan')
const schemaPath = join(planDir, '.schema.json')

let schema
try {
  schema = JSON.parse(readFileSync(schemaPath, 'utf8'))
} catch (error) {
  console.error(`${schemaPath}: cannot read the schema — ${error.message}`)
  process.exit(1)
}

const required = schema.required ?? []
const properties = schema.properties ?? {}

/**
 * The YAML frontmatter of a task file: a flat `key: value` block between
 * two `---` fences. The schema declares no nested values, so the flat
 * subset is the whole format.
 */
function parseFrontmatter(content, path) {
  const match = content.match(/^---\r?\n([\s\S]*?)\r?\n---(\r?\n|$)/)
  if (!match) {
    throw new Error('no YAML frontmatter (the file must start with `---`)')
  }
  const fields = new Map()
  for (const line of match[1].split(/\r?\n/)) {
    if (line.trim() === '' || line.trimStart().startsWith('#')) {
      continue
    }
    const separator = line.indexOf(':')
    if (separator === -1) {
      throw new Error(`malformed frontmatter line: ${line}`)
    }
    fields.set(line.slice(0, separator).trim(), line.slice(separator + 1).trim())
  }
  return fields
}

let failed = false

for (const name of readdirSync(planDir).filter((entry) => entry.endsWith('.md')).sort()) {
  const path = join(planDir, name)
  const shown = relative(root, path)

  let fields
  try {
    fields = parseFrontmatter(readFileSync(path, 'utf8'), path)
  } catch (error) {
    console.error(`${shown}: FAIL — ${error.message}`)
    failed = true
    continue
  }

  for (const field of required) {
    if (!fields.has(field)) {
      console.error(`${shown}: FAIL — missing required field '${field}'`)
      failed = true
    }
  }

  for (const [key, property] of Object.entries(properties)) {
    if (!fields.has(key)) {
      continue
    }
    const value = fields.get(key)
    if (property.enum) {
      if (!property.enum.includes(value)) {
        console.error(
          `${shown}: FAIL — '${key}' must be one of [${property.enum.join(', ')}], got '${value}'`
        )
        failed = true
      }
    } else if (property.type === 'string' && typeof value !== 'string') {
      console.error(`${shown}: FAIL — '${key}' must be a string, got ${value}`)
      failed = true
    }
  }

  for (const key of fields.keys()) {
    if (!required.includes(key) && !(key in properties)) {
      console.warn(`${shown}: WARNING — unknown frontmatter key '${key}'`)
    }
  }
}

process.exit(failed ? 1 : 0)
