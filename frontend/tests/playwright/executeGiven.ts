import * as fs from 'fs'
import * as path from 'path'
import { spawn } from 'child_process'
import { createHash, createHmac } from 'crypto'
import { GivenItem } from './types'
import type { APIRequestContext } from '@playwright/test'
import { type WorkerPaths } from './paths'
import { CoverageTracer } from './tracer'

let authToken: string | null = null

export function resetAuthToken(): void {
  authToken = null
}

function readCurrentPassword(configDir: string): string | null {
  try {
    const configPath = path.join(configDir, 'config.toml')
    const raw = fs.readFileSync(configPath, 'utf-8')
    const match = raw.match(/password\s*=\s*"([^"]+)"/)
    return match ? match[1] : null
  } catch {
    return null
  }
}

function authHeaders(token: string): Record<string, string> {
  return {
    Authorization: `Bearer ${token}`,
    'Content-Type': 'application/json',
    Cookie: `jwt=${token}`
  }
}

async function ensureAuthenticated(
  request: APIRequestContext,
  backendUrl: string,
  adminPassword: string
): Promise<string> {
  if (authToken) return authToken
  const res = await request.post(`${backendUrl}/post/authenticate`, {
    data: JSON.stringify(adminPassword),
    headers: { 'Content-Type': 'application/json' }
  })
  if (!res.ok()) throw new Error(`Auth failed: ${res.status()}`)
  authToken = String(await res.json())
  return authToken
}

interface PhotoManifestEntry {
  output: string
  format?: string
  width?: number
  height?: number
  exif_date?: string
  tags?: string[]
}

export interface GivenContext {
  vars: Record<string, string>
  namespace?: string
}

export function createGivenContext(namespace?: string): GivenContext {
  return { vars: {}, namespace }
}

function sanitizeNamespace(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
}

function qualifyPath(relativePath: string, ns?: string): string {
  if (!ns) return relativePath
  const prefix = sanitizeNamespace(ns)
  return path.join(prefix, relativePath)
}

function runTestImageBatch(manifest: PhotoManifestEntry[]): Promise<void> {
  return new Promise((resolve, reject) => {
    const proc = spawn('cargo', ['xtask', 'test-image', 'batch', '-'], {
      stdio: ['pipe', 'inherit', 'inherit']
    })
    proc.on('error', (err) => reject(new Error(`cargo xtask test-image: ${err.message}`)))
    proc.on('exit', (code) => {
      if (code === 0) resolve()
      else reject(new Error(`cargo xtask test-image batch exited with code ${code}`))
    })
    proc.stdin.write(JSON.stringify(manifest))
    proc.stdin.end()
  })
}

function tracedRequest(base: APIRequestContext, tracer: CoverageTracer): APIRequestContext {
  return new Proxy(base, {
    get(target, prop) {
      const orig = (target as any)[prop]
      if (typeof orig !== 'function') return orig
      if (['fetch', 'post', 'put', 'get', 'delete'].includes(prop as string)) {
        return async (...args: any[]) => {
          const url = typeof args[0] === 'string' ? args[0] : String(args[0])
          const path = new URL(url).pathname
          const method =
            prop === 'fetch' ? ((args[1] as any)?.method ?? 'GET') : (prop as string).toUpperCase()
          tracer.recordAPI(method, path)
          return orig.apply(target, args)
        }
      }
      return orig.bind(target)
    }
  })
}

export async function executeGiven(
  baseRequest: APIRequestContext,
  given: GivenItem[],
  ctx: GivenContext,
  tracer: CoverageTracer | undefined,
  overridePaths: WorkerPaths
): Promise<GivenContext> {
  const {
    IMAGE_HOME: imageHome,
    BACKEND_URL: backendUrl,
    ADMIN_PASSWORD: adminPassword,
    CONFIG_DIR: configDir
  } = overridePaths

  const request = tracer ? tracedRequest(baseRequest, tracer) : baseRequest
  const result: GivenContext = { vars: { ...ctx.vars }, namespace: ctx.namespace }
  const ns = ctx.namespace

  type SeedEntry = { type: 'dir_album' | 'photo'; qualifiedPath: string; id_as?: string }
  const seedEntries: SeedEntry[] = []
  const photoManifest: PhotoManifestEntry[] = []
  const postIndexMoves: { from: string; to: string }[] = []
  let knownJwtSecret: string | undefined = undefined

  for (const item of given) {
    if ('dir_album' in item && item.dir_album) {
      const ga = item as { dir_album: string; id_as?: string }
      const qualified = qualifyPath(ga.dir_album, ns)
      const dirPath = path.join(imageHome, qualified)
      fs.mkdirSync(dirPath, { recursive: true })
      seedEntries.push({ type: 'dir_album', qualifiedPath: qualified, id_as: ga.id_as })
      if (ga.id_as) {
        const ph = path.join(dirPath, '.__e2e_ph__.jpg')
        photoManifest.push({ output: ph })
      }
    }

    if ('photo' in item && item.photo) {
      const ph = item as {
        photo: string
        id_as?: string
        format?: string
        width?: number
        height?: number
        tags?: string[]
        exif_date?: string
      }
      const qualified = qualifyPath(ph.photo, ns)
      const filePath = path.join(imageHome, qualified)

      const entry: PhotoManifestEntry = { output: filePath }
      if (ph.format) entry.format = ph.format
      if (ph.width) entry.width = ph.width
      if (ph.height) entry.height = ph.height
      if (ph.tags) entry.tags = ph.tags
      if (ph.exif_date) entry.exif_date = ph.exif_date
      photoManifest.push(entry)
      seedEntries.push({ type: 'photo', qualifiedPath: qualified, id_as: ph.id_as })
    }

    if ('remove' in item && item.remove) {
      const qualified = qualifyPath(item.remove, ns)
      const filePath = path.join(imageHome, qualified)
      try {
        fs.unlinkSync(filePath)
      } catch {}
    }

    if ('move' in item) {
      const mv = item as { move: string; to: string }
      const fromQualified = qualifyPath(mv.move, ns)
      const toQualified = qualifyPath(mv.to, ns)
      postIndexMoves.push({ from: fromQualified, to: toQualified })
    }

    if ('config' in item && item.config) {
      const cfg = item as {
        config: {
          read_only_mode?: boolean
          password?: string
          auth_key?: string
          fs_notify_watcher?: boolean
        }
      }
      const token = await ensureAuthenticated(request, backendUrl, adminPassword)
      const headers = authHeaders(token)

      if (cfg.config.password !== undefined) {
        const oldPassword = readCurrentPassword(configDir)
        const pwdRes = await request.fetch(`${backendUrl}/put/config/password`, {
          method: 'PUT',
          headers,
          data: { password: cfg.config.password, oldPassword }
        })
        if (!pwdRes.ok()) {
          throw new Error(`Password update failed: ${pwdRes.status()} ${await pwdRes.text()}`)
        }
      }

      if (cfg.config.read_only_mode !== undefined) {
        const res = await request.fetch(`${backendUrl}/put/config`, {
          method: 'PUT',
          headers,
          data: { readOnlyMode: cfg.config.read_only_mode }
        })
        if (!res.ok()) {
          throw new Error(`Config update failed: ${res.status()} ${await res.text()}`)
        }
      }

      if (cfg.config.fs_notify_watcher !== undefined) {
        const res = await request.fetch(`${backendUrl}/put/config`, {
          method: 'PUT',
          headers,
          data: { fsNotifyWatcher: cfg.config.fs_notify_watcher }
        })
        if (!res.ok()) {
          throw new Error(`Config update failed: ${res.status()} ${await res.text()}`)
        }
      }

      if (cfg.config.auth_key !== undefined) {
        const res = await request.fetch(`${backendUrl}/put/config`, {
          method: 'PUT',
          headers,
          data: { authKey: cfg.config.auth_key }
        })
        if (!res.ok()) {
          throw new Error(`Auth key update failed: ${res.status()} ${await res.text()}`)
        }
        knownJwtSecret = cfg.config.auth_key
        authToken = null
      }
    }

    if ('photo_raw' in item && item.photo_raw) {
      const pr = item as {
        photo_raw: string
        format?: string
        width?: number
        height?: number
      }
      const qualified = qualifyPath(pr.photo_raw, ns)
      const filePath = path.join(imageHome, qualified)
      const entry: PhotoManifestEntry = { output: filePath }
      if (pr.format) entry.format = pr.format
      if (pr.width) entry.width = pr.width
      if (pr.height) entry.height = pr.height
      photoManifest.push(entry)
      // Deliberately NOT added to seedEntries — file is placed on disk but
      // no auto-scan is triggered, leaving it unindexed for scan-flow tests.
    }
  }

  if (photoManifest.length > 0) {
    await runTestImageBatch(photoManifest)
  }

  if (seedEntries.length > 0) {
    const token = await ensureAuthenticated(request, backendUrl, adminPassword)
    const allHeaders = authHeaders(token)

    const indexRes = await request.fetch(`${backendUrl}/post/index/album`, {
      method: 'POST',
      headers: allHeaders,
      data: { album: '/' }
    })
    if (!indexRes.ok()) {
      throw new Error(`Index failed: ${indexRes.status()} ${await indexRes.text()}`)
    }

    let indexed = false
    for (let i = 0; i < 60; i++) {
      const statusRes = await request.fetch(`${backendUrl}/get/index/status`, {
        headers: allHeaders
      })
      if (statusRes.ok()) {
        const status = await statusRes.json()
        if (status.state === 'idle' || status.state === 'completed') {
          indexed = true
          break
        }
      }
      await new Promise((r) => setTimeout(r, 1000))
    }
    if (!indexed) throw new Error('Index did not complete within 60s')

    const wantsIdAs = seedEntries.some((e) => e.id_as !== undefined)
    if (wantsIdAs) {
      const dataRes = await request.fetch(`${backendUrl}/get/get-data?start=0&end=100`, {
        headers: allHeaders
      })
      const data = (await dataRes.json()) as any[]

      for (const entry of seedEntries) {
        if (!entry.id_as) continue
        if (entry.type === 'dir_album') {
          const albumId = await findAlbum(request, allHeaders, backendUrl, entry.qualifiedPath)
          if (albumId) result.vars[entry.id_as] = albumId
        } else if (entry.type === 'photo') {
          const hash = findPhotoHash(entry.qualifiedPath, data)
          if (hash) result.vars[entry.id_as] = hash
        }
      }
    }
  }

  for (const mv of postIndexMoves) {
    const fromPath = path.join(imageHome, mv.from)
    const toPath = path.join(imageHome, mv.to)

    const fileBuffer = fs.readFileSync(fromPath)
    const hexHash = createHash('sha256').update(fileBuffer).digest('hex')

    fs.mkdirSync(path.dirname(toPath), { recursive: true })
    try {
      fs.renameSync(fromPath, toPath)
    } catch {
      throw new Error(`Failed to move ${fromPath} to ${toPath}`)
    }

    const token = await ensureAuthenticated(request, backendUrl, adminPassword)
    if (knownJwtSecret) {
      const ext = path.extname(fromPath).slice(1) || 'jpg'
      const ts = Math.floor(Date.now() / 1000)
      const hashJwt = createHashJwt(
        {
          allowOriginal: true,
          hash: hexHash,
          timestamp: ts,
          exp: ts + 300
        },
        knownJwtSecret!
      )

      const url = `${backendUrl}/object/imported/${hexHash.slice(0, 2)}/${hexHash}.${ext}?updated_at=0`
      const dlRes = await request.fetch(url, {
        headers: {
          Authorization: `Bearer ${hashJwt}`,
          Cookie: `jwt=${token}`
        }
      })
      if (dlRes.ok()) {
        throw new Error(
          `Expected import download to fail after file move, but got ${dlRes.status()} for ${url}`
        )
      }
    }
  }

  return result
}

async function findAlbum(
  request: APIRequestContext,
  headers: Record<string, string>,
  backendUrl: string,
  qualifiedPath: string
): Promise<string | null> {
  const res = await request.fetch(`${backendUrl}/get/get-albums`, { headers })
  if (!res.ok()) return null
  const albums = (await res.json()) as any[]
  const match = albums.find((a: any) => a.dirPath && a.dirPath.endsWith(qualifiedPath))
  return match ? String(match.album_id) : null
}

function findPhotoHash(qualifiedPath: string, data: any[]): string | null {
  const match = data.find((d: any) => {
    const alias = d.abstractData?.currentAlias?.filePath
    return alias && alias.endsWith(qualifiedPath)
  })
  return match ? String(match.hash) : null
}

function base64urlEncode(data: Buffer): string {
  return data.toString('base64').replace(/=/g, '').replace(/\+/g, '-').replace(/\//g, '_')
}

function createHashJwt(payload: Record<string, unknown>, secret: string): string {
  const header = base64urlEncode(Buffer.from(JSON.stringify({ alg: 'HS256', typ: 'JWT' })))
  const body = base64urlEncode(Buffer.from(JSON.stringify(payload)))
  const signature = createHmac('sha256', secret).update(`${header}.${body}`).digest()
  return `${header}.${body}.${base64urlEncode(signature)}`
}

function readJwtSecretFromConfig(configDir: string): string {
  try {
    const configPath = path.join(configDir, 'config.toml')
    const raw = fs.readFileSync(configPath, 'utf-8')
    const match = raw.match(/auth_key\s*=\s*"([^"]+)"/)
    if (match) return match[1]
  } catch {
    // fall through
  }
  return 'change_me'
}
