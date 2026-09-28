import { createServer } from 'net'
import { fileURLToPath } from 'url'
import * as path from 'path'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

// frontend/tests/playwright/ → frontend/tests/ → frontend/ → repo root
/// Repository root, the base the capability manifest's fixture paths are
/// recorded against (see `pinnedFixtures.ts`).
export const REPO_ROOT: string = path.resolve(__dirname, '..', '..', '..')

/// Top-level directory for test run outputs (reports, artifacts, per-scenario backends).
/// Override with `TEST_DIR` env var. Defaults to `.testruns/` under the repo root.
export const TEST_DIR: string = process.env.TEST_DIR ?? path.resolve(REPO_ROOT, '.testruns')

export const FRONTEND_URL = 'http://localhost:5173'
export const ADMIN_PASSWORD = 'e2e_test_pwd'

/// First port of the random draw, and the width of the range. Workers pick
/// from 30000 upwards, so nothing below 30000 is a candidate.
const PORT_RANGE_START = 30000
const PORT_RANGE_SIZE = 30000

/// How many candidate ports to try before giving up. The random draw over a
/// 30000-wide range makes a second attempt almost never necessary; the bound
/// exists so a machine that has the whole range occupied fails with a clear
/// error instead of spinning.
export const BACKEND_PORT_PROBE_ATTEMPTS = 10

export interface WorkerPaths {
  DIR: string
  CONFIG_DIR: string
  DATA_DIR: string
  IMAGE_HOME: string
  BACKEND_PORT: number
  BACKEND_URL: string
  ADMIN_PASSWORD: string
}

/// Report whether `port` can be bound, by binding it and letting it go again.
///
/// The probe listens on the wildcard address, which is what the backend binds
/// by default (`AppConfig::address` is `0.0.0.0`). Measured: a listener held
/// only on `127.0.0.1` still refuses the backend's wildcard bind, so a
/// wildcard probe is the conservative side of that asymmetry — it can miss a
/// holder bound to some third address, but never the reverse.
export function isPortFree(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const probe = createServer()
    const settle = (free: boolean) => {
      // `close` is a no-op once the socket is already closed, so this is safe
      // on the error path as well.
      probe.close(() => resolve(free))
    }
    probe.once('error', () => settle(false))
    probe.listen({ port, exclusive: true }, () => settle(true))
  })
}

/// The port worker `workerNum` is given: `30000 + NUM*2`, unique per worker
/// because no two workers share a number within one run.
export function drawWorkerPort(workerNum: number): number {
  return 30000 + workerNum * 2
}

/// A random port in 30000–59999.
export function drawRandomPort(): number {
  return PORT_RANGE_START + Math.floor(Math.random() * PORT_RANGE_SIZE)
}

/// Draw a port from `draw` until `probe` reports it free, and return it.
///
/// `draw` receives the zero-based attempt index, so a redraw can be a fresh
/// random port or a walk along a deterministic range. At most `attempts`
/// candidates are tried; exhausting them is an error rather than a loop, and
/// the message names the ports that were rejected so the collision is visible
/// in the test output.
///
/// The probe is advisory, not a reservation: the port is released before the
/// backend binds it, so a holder that appears in between still loses. It
/// narrows the window from "any of 30000 ports, drawn blind" to "one port,
/// checked moments before the bind".
export async function findFreePort(
  draw: (attempt: number) => number,
  probe: (port: number) => Promise<boolean> = isPortFree,
  attempts: number = BACKEND_PORT_PROBE_ATTEMPTS
): Promise<number> {
  const rejected: number[] = []
  for (let attempt = 0; attempt < attempts; attempt++) {
    const port = draw(attempt)
    if (await probe(port)) return port
    rejected.push(port)
  }
  throw new Error(`no free backend port in ${attempts} attempts; rejected: ${rejected.join(', ')}`)
}

/** Generate a fresh set of paths for an isolated backend instance.
 *
 *  Directory layout: `{TEST_DIR}/playwright-{ID}/`
 *  When `WORKER_NUM` is set, `ID` is the worker number for deterministic
 *  paths and the first port candidate is `30000 + NUM*2` (see
 *  [`drawWorkerPort`]). Otherwise `ID` is a random 6-char hex string. Either
 *  way the port is probed and redrawn on a collision — see
 *  [`findFreePort`], which is why this is async: probing costs a round trip
 *  through the kernel. */
export async function createPaths(): Promise<WorkerPaths> {
  const workerNum = process.env.WORKER_NUM
  if (workerNum !== undefined) {
    const num = parseInt(workerNum, 10)
    if (!isNaN(num)) {
      const dir = path.resolve(TEST_DIR, `playwright-${num}`)
      const port = await findFreePort(
        (attempt) => (attempt === 0 ? drawWorkerPort(num) : drawRandomPort()),
        isPortFree,
        BACKEND_PORT_PROBE_ATTEMPTS
      )
      return workerPaths(dir, port)
    }
  }
  const runId = Math.random().toString(36).slice(2, 8)
  const dir = path.resolve(TEST_DIR, `playwright-${runId}`)
  const port = await findFreePort(() => drawRandomPort())
  return workerPaths(dir, port)
}

function workerPaths(dir: string, port: number): WorkerPaths {
  return {
    DIR: dir,
    CONFIG_DIR: path.join(dir, 'config'),
    DATA_DIR: path.join(dir, 'data'),
    IMAGE_HOME: path.join(dir, 'data', 'images'),
    BACKEND_PORT: port,
    BACKEND_URL: `http://localhost:${port}`,
    ADMIN_PASSWORD: ADMIN_PASSWORD
  }
}
