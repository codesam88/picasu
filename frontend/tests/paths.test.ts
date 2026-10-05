import { createServer, type Server } from 'net'
import * as path from 'path'
import { afterEach, describe, expect, it } from 'vitest'
import {
  BACKEND_PORT_PROBE_ATTEMPTS,
  TEST_DIR,
  createPaths,
  drawRandomPort,
  drawWorkerPort,
  findFreePort,
  isPortFree
} from './playwright/paths'

/// Hold `port` open for the duration of the returned releaser, so a test can
/// present the probe with a port that is genuinely in use.
function holdPort(port: number): Promise<{ releaser: () => Promise<void> }> {
  return new Promise((resolve, reject) => {
    const server: Server = createServer()
    server.once('error', reject)
    // Listen on loopback only, the way an unrelated local service would: the
    // backend binds the wildcard, and a loopback holder still refuses it.
    server.listen({ port, host: '127.0.0.1' }, () => {
      resolve({
        releaser: () => new Promise<void>((done) => server.close(() => done()))
      })
    })
  })
}

/// A port nothing holds. The kernel picks it, so the port is closed at the
/// moment the server releases it.
async function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer()
    server.once('error', reject)
    server.listen(0, () => {
      const address = server.address()
      if (address === null || typeof address === 'string') {
        server.close(() => reject(new Error('no tcp address')))
        return
      }
      const { port } = address
      server.close(() => resolve(port))
    })
  })
}

describe('isPortFree', () => {
  const releasers: (() => Promise<void>)[] = []
  afterEach(async () => {
    await Promise.all(releasers.splice(0).map((release) => release()))
  })

  it('reports a port it can bind as free', async () => {
    const port = await freePort()
    await expect(isPortFree(port)).resolves.toBe(true)
  })

  it('reports a port held by another listener as taken', async () => {
    const port = await freePort()
    releasers.push((await holdPort(port)).releaser)
    await expect(isPortFree(port)).resolves.toBe(false)
  })

  it('releases the probe socket, so the port is still bindable afterwards', async () => {
    const port = await freePort()
    await expect(isPortFree(port)).resolves.toBe(true)
    // A leaked probe socket would make this throw EADDRINUSE.
    const { releaser } = await holdPort(port)
    releasers.push(releaser)
  })
})

describe('findFreePort', () => {
  it('returns the first candidate when the probe reports it free', async () => {
    const probed: number[] = []
    const port = await findFreePort(
      () => 31000,
      async (candidate) => {
        probed.push(candidate)
        return true
      }
    )
    expect(port).toBe(31000)
    expect(probed).toEqual([31000])
  })

  it('redraws on a collision and returns the first free draw', async () => {
    const taken = new Set([31001, 31002])
    const probed: number[] = []
    const port = await findFreePort(
      (attempt) => 31001 + attempt,
      async (candidate) => {
        probed.push(candidate)
        return !taken.has(candidate)
      }
    )
    expect(port).toBe(31003)
    expect(probed).toEqual([31001, 31002, 31003])
  })

  it('passes the attempt index to the draw so each retry can differ', async () => {
    const attempts: number[] = []
    await findFreePort(
      (attempt) => {
        attempts.push(attempt)
        return 32000 + attempt
      },
      async () => attempts.length >= 3
    )
    expect(attempts).toEqual([0, 1, 2])
  })

  it('gives up after the bounded attempt count instead of looping', async () => {
    let calls = 0
    await expect(
      findFreePort(
        () => 33000 + calls++,
        async () => false,
        4
      )
    ).rejects.toThrow(/4/)
    expect(calls).toBe(4)
  })

  it('names the rejected ports in the error so the collision is diagnosable', async () => {
    await expect(
      findFreePort(
        (attempt) => 34000 + attempt,
        async () => false,
        2
      )
    ).rejects.toThrow(/34000, 34001/)
  })
})

describe('port candidates', () => {
  it('derives the worker port from the worker number', () => {
    expect(drawWorkerPort(0)).toBe(30000)
    expect(drawWorkerPort(3)).toBe(30006)
  })

  it('draws the random port inside the documented 30000-59999 range', () => {
    for (let i = 0; i < 500; i++) {
      const port = drawRandomPort()
      expect(port).toBeGreaterThanOrEqual(30000)
      expect(port).toBeLessThan(60000)
    }
  })
})

describe('createPaths', () => {
  const savedWorkerNum = process.env.WORKER_NUM
  afterEach(() => {
    if (savedWorkerNum === undefined) delete process.env.WORKER_NUM
    else process.env.WORKER_NUM = savedWorkerNum
  })

  it('returns a port that probes free, and a URL that matches it', async () => {
    delete process.env.WORKER_NUM
    const paths = await createPaths()
    expect(await isPortFree(paths.BACKEND_PORT)).toBe(true)
    expect(paths.BACKEND_URL).toBe(`http://localhost:${paths.BACKEND_PORT}`)
  })

  it('derives the directory and port from WORKER_NUM when it is set', async () => {
    process.env.WORKER_NUM = '4'
    const paths = await createPaths()
    // `TEST_DIR` is read when the module loads, so the directory is asserted
    // against the exported value rather than an env var set here.
    expect(paths.DIR).toBe(path.resolve(TEST_DIR, 'playwright-4'))
    expect(paths.DATA_DIR).toBe(path.join(paths.DIR, 'data'))
    // The documented formula, unless 30008 was not free — in which case the
    // probe redrew, and the port still probes free.
    const free = await isPortFree(30008)
    expect(paths.BACKEND_PORT === 30008 || !free).toBe(true)
    expect(await isPortFree(paths.BACKEND_PORT)).toBe(true)
  })

  it('defaults to the bounded attempt count, so the constant and the loop agree', async () => {
    let calls = 0
    await expect(
      findFreePort(
        () => 35000 + calls++,
        async () => false
      )
    ).rejects.toThrow(new RegExp(`in ${BACKEND_PORT_PROBE_ATTEMPTS} attempts`))
    expect(calls).toBe(BACKEND_PORT_PROBE_ATTEMPTS)
  })
})
