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
const SHUTDOWN_GRACE_MS = 10_000

/// Children this process started and has not reaped yet. The teardown backstop
/// below walks this set, so it is the single answer to "what is still running
/// that we own".
const liveBackends = new Set<ChildProcess>()

/// Send `signal` and resolve once the child is gone.
///
/// A backend that has already exited resolves immediately. Without that check
/// the `exit` event has already fired and the promise can only be settled by
/// the grace timer — the caller waits out the full grace period for a process
/// that died during startup.
///
/// SIGTERM alone is not enough: measured on the current binary, a backend
/// whose bind failed stays alive with no listener, and a backend interrupted
/// mid-shutdown has been observed alive with its listener already closed. The
/// grace period then escalates to SIGKILL, which cannot be caught.
function terminate(proc: ChildProcess, graceMs: number = SHUTDOWN_GRACE_MS): Promise<void> {
  if (proc.exitCode !== null || proc.signalCode !== null) return Promise.resolve()
  return new Promise<void>((resolve) => {
    const escalate = setTimeout(() => {
      proc.kill('SIGKILL')
    }, graceMs)
    proc.once('exit', () => {
      clearTimeout(escalate)
      liveBackends.delete(proc)
      resolve()
    })
    proc.kill('SIGTERM')
  })
}

/// SIGKILL every child still running. Synchronous on purpose: an `exit`
/// handler may not await anything, and `ChildProcess.kill` is only a signal
/// send, so this is the one escalation that still works while the worker is
/// on its way out.
function reapLiveBackends(): void {
  for (const proc of liveBackends) {
    try {
      proc.kill('SIGKILL')
    } catch {
      // Already reaped, or the handle is otherwise dead — nothing to do.
    }
  }
  liveBackends.clear()
}

/// A SIGKILL aimed at this process is not coverable from here: nothing in the
/// process gets to run. The port probe is what keeps such a survivor from
/// affecting the next run.
let backstopInstalled = false

/// Install the backstop on every way this process can end while a backend is
/// still up.
///
/// The measured leak: a worker killed outright (CI cancel, an OOM kill, a
/// runner force-kill) never reaches the fixture's `stop()`, so its child is
/// reparented to init and goes on serving the port. Measured on the same
/// binary: SIGTERM from a live worker reaps the child in under a second, so
/// the leak is the missing signal, not a signal the backend ignores.
///
/// `exit` covers a normal return, an uncaught throw and an explicit
/// `process.exit`; `beforeExit` covers a drained event loop with a forgotten
/// handle. Neither runs for a fatal signal, so those are handled here: we reap,
/// and only re-raise when nothing else wants the signal, so the process still
/// dies of it and keeps the "killed by signal" status. Where another handler
/// exists — the Playwright worker installs its own for the signals it treats as
/// an abort — we stay out of its way.
///
/// Installed once per process: a worker starts a backend per scenario, and
/// re-registering would pile up listeners until Node warns about it.
function installBackstop(): void {
  if (backstopInstalled) return
  backstopInstalled = true

  process.on('exit', reapLiveBackends)
  process.on('beforeExit', reapLiveBackends)
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP'] as const) {
    const handler = () => {
      reapLiveBackends()
      if (process.listenerCount(signal) === 1) {
        process.removeListener(signal, handler)
        process.kill(process.pid, signal)
      }
    }
    process.on(signal, handler)
  }
}

function waitForServer(url: string, proc: ChildProcess, stderrTail: () => string): Promise<void> {
  const start = Date.now()
  return new Promise((resolve, reject) => {
    let done = false
    const finish = (err?: Error) => {
      if (done) return
      done = true
      if (err) reject(err)
      else resolve()
    }

    // Fail immediately if the process exits before the server is ready.
    proc.once('exit', (code, signal) => {
      finish(new Error(`Backend exited before becoming ready (code=${code}, signal=${signal})`))
    })

    function poll() {
      if (done) return
      const req = http.get(url, (res) => {
        res.resume()
        // A backend that cannot bind its port logs the refusal and stays up
        // without listening, so the process is not the signal here — the
        // answer to this poll came from whatever else holds the port. That
        // would hand the scenario another worker's backend, silently, so the
        // refusal is turned into a startup failure instead.
        if (/Address already in use/i.test(stderrTail())) {
          finish(
            new Error(
              `Backend at ${url} could not bind its port: Address already in use. ` +
                `Another process is holding ${url}; give the run a free port range (see ` +
                `paths.ts findFreePort) or stop the other process.`
            )
          )
          return
        }
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

  // Without PICASU_BINARY the child is `cargo run`, which execs the backend
  // rather than forking it (measured: the pid holding the port is the pid
  // spawn returned). The handle below is therefore the backend itself, so the
  // backstop and `stop()` both reach it.
  liveBackends.add(proc)
  installBackstop()

  // Bounded ring of recent stderr, so a startup failure can report why.
  const STDERR_TAIL_LINES = 200
  const stderrLines: string[] = []
  const stderrTail = () => stderrLines.join('\n')

  proc.stdout?.on('data', (chunk: Buffer) => {
    for (const line of chunk.toString().split('\n').filter(Boolean)) {
      process.stdout.write(`${logTag} ${line}\n`)
    }
  })
  proc.stderr?.on('data', (chunk: Buffer) => {
    const text = chunk.toString()
    for (const line of text.split('\n').filter(Boolean)) {
      stderrLines.push(line)
      process.stderr.write(`${logTag} ${line}\n`)
    }
    if (stderrLines.length > STDERR_TAIL_LINES) {
      stderrLines.splice(0, stderrLines.length - STDERR_TAIL_LINES)
    }
  })

  proc.on('exit', (code) => {
    liveBackends.delete(proc)
    if (code !== 0 && code !== null) {
      process.stderr.write(`${logTag} exited with code ${code}\n`)
    }
  })

  try {
    await waitForServer(paths.BACKEND_URL, proc, stderrTail)
  } catch (err) {
    // Startup failed, so no handle reaches the caller and nothing else will
    // ever ask this child to stop. Measured: the backend stays alive after a
    // refused bind, so this is a real child to reap, not a formality.
    await terminate(proc)
    throw err
  }

  return {
    stop: () => terminate(proc)
  }
}
