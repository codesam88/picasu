import { spawn, ChildProcess } from 'child_process'
import * as http from 'http'
import * as path from 'path'
import * as fs from 'fs'
import { fileURLToPath } from 'url'
import type { WorkerPaths } from './paths'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const BACKEND_DIR = path.resolve(__dirname, '..', '..', '..', 'backend')

const POLL_INTERVAL_MS = 200
const STARTUP_TIMEOUT_MS = 120_000

function waitForServer(url: string, proc: ChildProcess): Promise<void> {
  const start = Date.now()
  return new Promise((resolve, reject) => {
    let done = false
    const finish = (err?: Error) => {
      if (done) return
      done = true
      err ? reject(err) : resolve()
    }

    // Fail immediately if the process exits before the server is ready.
    proc.once('exit', (code, signal) => {
      finish(new Error(`Backend exited before becoming ready (code=${code}, signal=${signal})`))
    })

    function poll() {
      if (done) return
      const req = http.get(url, (res) => {
        res.resume()
        finish()
      })
      req.on('error', () => {
        if (done) return
        if (Date.now() - start > STARTUP_TIMEOUT_MS) {
          finish(new Error(`Backend at ${url} did not start within ${STARTUP_TIMEOUT_MS}ms`))
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

const REPO_ROOT = path.resolve(__dirname, '..', '..', '..')

export async function startBackend(paths: WorkerPaths): Promise<BackendHandle> {
  fs.mkdirSync(paths.DATA_DIR, { recursive: true })
  fs.mkdirSync(paths.CONFIG_DIR, { recursive: true })

  const binaryPath = process.env.PICASU_BINARY
  const [cmd, cmdArgs, cmdOpts] = binaryPath
    ? [path.resolve(REPO_ROOT, binaryPath), [], {}]
    : ['cargo', ['run', '--bin', 'picasu'], { cwd: BACKEND_DIR }]

  const logTag = `[${path.basename(paths.DIR)}]`
  const proc = spawn(cmd, cmdArgs, {
    ...cmdOpts,
    env: {
      ...process.env,
      PICASU_PORT: String(paths.BACKEND_PORT),
      PICASU_DATA_HOME: paths.DATA_DIR,
      PICASU_CONFIG_HOME: paths.CONFIG_DIR,
      PICASU_WEB_ROOT: path.resolve(REPO_ROOT, 'frontend/dist')
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

  await waitForServer(paths.BACKEND_URL, proc)

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
