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

export interface WorkerPaths {
  DIR: string
  CONFIG_DIR: string
  DATA_DIR: string
  IMAGE_HOME: string
  BACKEND_PORT: number
  BACKEND_URL: string
  ADMIN_PASSWORD: string
}

/** Generate a fresh set of paths for an isolated backend instance.
 *
 *  Directory layout: `{TEST_DIR}/playwright-{ID}/`
 *  When `WORKER_NUM` is set, `ID` is the worker number for deterministic
 *  paths and ports (port = `30000 + NUM*2`). Otherwise a random 6-char hex
 *  string is used. */
export function createPaths(): WorkerPaths {
  const workerNum = process.env.WORKER_NUM
  if (workerNum !== undefined) {
    const num = parseInt(workerNum, 10)
    if (!isNaN(num)) {
      const dir = path.resolve(TEST_DIR, `playwright-${num}`)
      const port = 30000 + num * 2
      return {
        DIR: dir,
        CONFIG_DIR: path.join(dir, 'config'),
        DATA_DIR: path.join(dir, 'data'),
        IMAGE_HOME: path.join(dir, 'data', 'images'),
        BACKEND_PORT: port,
        BACKEND_URL: `http://localhost:${port}`,
        ADMIN_PASSWORD: 'e2e_test_pwd'
      }
    }
  }
  const runId = Math.random().toString(36).slice(2, 8)
  const dir = path.resolve(TEST_DIR, `playwright-${runId}`)
  const port = 30000 + Math.floor(Math.random() * 30000)
  return {
    DIR: dir,
    CONFIG_DIR: path.join(dir, 'config'),
    DATA_DIR: path.join(dir, 'data'),
    IMAGE_HOME: path.join(dir, 'data', 'images'),
    BACKEND_PORT: port,
    BACKEND_URL: `http://localhost:${port}`,
    ADMIN_PASSWORD: 'e2e_test_pwd'
  }
}
