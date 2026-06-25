import { spawn, ChildProcess } from 'child_process'
import * as http from 'http'
import * as path from 'path'
import * as fs from 'fs'
import { fileURLToPath } from 'url'
import type { WorkerPaths } from './paths'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const BACKEND_DIR = path.resolve(__dirname, '..', '..', '..', 'gallery-backend')

const POLL_INTERVAL_MS = 200
const STARTUP_TIMEOUT_MS = 120_000

function waitForServer(url: string): Promise<void> {
  const start = Date.now()
  return new Promise((resolve, reject) => {
    function poll() {
      const req = http.get(url, (res) => {
        res.resume()
        resolve()
      })
      req.on('error', () => {
        if (Date.now() - start > STARTUP_TIMEOUT_MS) {
          reject(new Error(`Backend at ${url} did not start within ${STARTUP_TIMEOUT_MS}ms`))
        } else {
          setTimeout(poll, POLL_INTERVAL_MS)
        }
      })
      req.end()
    }
    poll()
  })
}

export interface BackendHandle {
  stop(): Promise<void>
}

export async function startBackend(paths: WorkerPaths): Promise<BackendHandle> {
  fs.mkdirSync(paths.DATA_DIR, { recursive: true })
  fs.mkdirSync(paths.CONFIG_DIR, { recursive: true })

  const logTag = `[${path.basename(paths.DIR)}]`
  const proc = spawn('cargo', ['run', '--bin', 'urocissa'], {
    cwd: BACKEND_DIR,
    env: {
      ...process.env,
      UROCISSA_PORT: String(paths.BACKEND_PORT),
      UROCISSA_DATA_HOME: paths.DATA_DIR,
      UROCISSA_CONFIG_HOME: paths.CONFIG_DIR
    },
    stdio: ['ignore', 'pipe', 'pipe']
  })

  proc.stdout?.on('data', (chunk: Buffer) => {
    for (const line of chunk.toString().split('\n').filter(Boolean)) {
      process.stdout.write(`${logTag} ${line}\n`)
    }
  })
  proc.stderr?.on('data', (chunk: Buffer) => {
    for (const line of chunk.toString().split('\n').filter(Boolean)) {
      process.stderr.write(`${logTag} ${line}\n`)
    }
  })

  proc.on('exit', (code) => {
    if (code !== 0 && code !== null) {
      process.stderr.write(`${logTag} exited with code ${code}\n`)
    }
  })

  await waitForServer(paths.BACKEND_URL)

  return {
    stop: async () => {
      proc.kill('SIGTERM')
      await new Promise<void>((resolve) => {
        const timeout = setTimeout(() => {
          proc.kill('SIGKILL')
          resolve()
        }, 10_000)
        proc.on('exit', () => {
          clearTimeout(timeout)
          resolve()
        })
      })
    }
  }
}
