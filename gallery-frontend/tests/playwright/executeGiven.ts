import * as fs from 'fs'
import * as path from 'path'
import { GivenItem } from './types'
import type { APIRequestContext } from '@playwright/test'
import { IMAGE_HOME, BACKEND_URL, ADMIN_PASSWORD } from './paths'

let authToken: string | null = null

async function ensureAuthenticated(request: APIRequestContext): Promise<string> {
  if (authToken) return authToken
  const res = await request.post(`${BACKEND_URL}/post/authenticate`, {
    data: ADMIN_PASSWORD
  })
  if (!res.ok()) throw new Error(`Auth failed: ${res.status()}`)
  authToken = String(await res.json())
  return authToken
}

const MINIMAL_JPEG = Buffer.from([
  0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
  0x00, 0x01, 0x00, 0x00, 0xff, 0xdb, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08,
  0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0a, 0x0c, 0x14, 0x0d, 0x0c, 0x0b, 0x0b, 0x0c, 0x19, 0x12,
  0x13, 0x0f, 0x14, 0x1d, 0x1a, 0x1f, 0x1e, 0x1d, 0x1a, 0x1c, 0x1c, 0x20, 0x24, 0x2e, 0x27, 0x20,
  0x22, 0x2c, 0x23, 0x1c, 0x1c, 0x28, 0x37, 0x29, 0x2c, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1f, 0x27,
  0x39, 0x3d, 0x38, 0x32, 0x3c, 0x2e, 0x33, 0x34, 0x32, 0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x01,
  0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xff, 0xc4, 0x00, 0x1f, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01,
  0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05,
  0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0xff, 0xc4, 0x00, 0xb5, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03,
  0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x11,
  0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08,
  0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a,
  0x16, 0xe1, 0xff, 0xd9
])

export interface GivenContext {
  vars: Record<string, string>
  namespace?: string
}

export function createGivenContext(namespace?: string): GivenContext {
  return { vars: {}, namespace }
}

function sanitizeNamespace(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')
}

function qualifyPath(relativePath: string, ns?: string): string {
  if (!ns) return relativePath
  const prefix = sanitizeNamespace(ns)
  return path.join(prefix, relativePath)
}

export async function executeGiven(
  request: APIRequestContext,
  given: GivenItem[],
  ctx: GivenContext
): Promise<GivenContext> {
  const result: GivenContext = { vars: { ...ctx.vars }, namespace: ctx.namespace }
  const ns = ctx.namespace

  type SeedEntry = { type: 'dir_album' | 'photo'; qualifiedPath: string; id_as?: string }
  const seedEntries: SeedEntry[] = []

  for (const item of given) {
    if ('dir_album' in item && item.dir_album) {
      const ga = item as { dir_album: string; id_as?: string }
      const qualified = qualifyPath(ga.dir_album, ns)
      const dirPath = path.join(IMAGE_HOME, qualified)
      fs.mkdirSync(dirPath, { recursive: true })
      seedEntries.push({ type: 'dir_album', qualifiedPath: qualified, id_as: ga.id_as })
      if (ga.id_as) {
        const ph = path.join(dirPath, '.__e2e_ph__.jpg')
        fs.writeFileSync(ph, MINIMAL_JPEG)
      }
    }

    if ('photo' in item && item.photo) {
      const ph = item as { photo: string; id_as?: string }
      const qualified = qualifyPath(ph.photo, ns)
      const filePath = path.join(IMAGE_HOME, qualified)
      fs.mkdirSync(path.dirname(filePath), { recursive: true })
      fs.writeFileSync(filePath, MINIMAL_JPEG)
      seedEntries.push({ type: 'photo', qualifiedPath: qualified, id_as: ph.id_as })
    }

    if ('remove' in item && item.remove) {
      const qualified = qualifyPath(item.remove, ns)
      const filePath = path.join(IMAGE_HOME, qualified)
      try {
        fs.unlinkSync(filePath)
      } catch {}
    }

    if ('config' in item && item.config) {
      const cfg = item as { config: { read_only_mode?: boolean } }
      const token = await ensureAuthenticated(request)
      const res = await request.fetch(`${BACKEND_URL}/put/config`, {
        method: 'PUT',
        headers: {
          Authorization: `Bearer ${token}`,
          'Content-Type': 'application/json'
        },
        data: { readOnlyMode: cfg.config.read_only_mode }
      })
      if (!res.ok()) {
        throw new Error(
          `Config update failed: ${res.status()} ${await res.text()}`
        )
      }
    }
  }

  if (seedEntries.length > 0) {
    const token = await ensureAuthenticated(request)
    const authHeaders = { Authorization: `Bearer ${token}` }

    const indexRes = await request.fetch(`${BACKEND_URL}/post/index/album`, {
      method: 'POST',
      headers: { ...authHeaders, 'Content-Type': 'application/json' },
      data: { album: '/' }
    })
    if (!indexRes.ok()) {
      throw new Error(`Index failed: ${indexRes.status()} ${await indexRes.text()}`)
    }

    let indexed = false
    for (let i = 0; i < 60; i++) {
      const statusRes = await request.fetch(`${BACKEND_URL}/get/index/status`, {
        headers: authHeaders
      })
      if (statusRes.ok()) {
        const status = await statusRes.json()
        if (status.indexing === false) {
          indexed = true
          break
        }
      }
      await new Promise((r) => setTimeout(r, 1000))
    }
    if (!indexed) throw new Error('Index did not complete within 60s')

    const wantsIdAs = seedEntries.some((e) => e.id_as !== undefined)
    if (wantsIdAs) {
      const dataRes = await request.fetch(`${BACKEND_URL}/get/get-data?start=0&end=100`, {
        headers: authHeaders
      })
      const data = (await dataRes.json()) as any[]

      for (const entry of seedEntries) {
        if (!entry.id_as) continue
        if (entry.type === 'dir_album') {
          const albumId = await findAlbum(request, authHeaders, entry.qualifiedPath)
          if (albumId) result.vars[entry.id_as] = albumId
        } else if (entry.type === 'photo') {
          const hash = findPhotoHash(entry.qualifiedPath, data)
          if (hash) result.vars[entry.id_as] = hash
        }
      }
    }
  }

  return result
}

async function findAlbum(
  request: APIRequestContext,
  headers: Record<string, string>,
  qualifiedPath: string
): Promise<string | null> {
  const res = await request.fetch(`${BACKEND_URL}/get/get-albums`, { headers })
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
