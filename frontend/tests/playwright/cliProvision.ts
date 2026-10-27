import { spawn } from 'child_process'
import * as fs from 'fs'
import * as path from 'path'
import { fileURLToPath } from 'url'
import type { WorkerPaths } from './paths'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
// frontend/tests/playwright/ → frontend/tests/ → frontend/ → repo root
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..')

export interface CliUserDecl {
  user_id: string
  password: string
  admin?: boolean
}

/// Offline user provisioning (phase A of the scenario harness): creates the
/// declared users in the scenario's user store *before* the backend starts,
/// via the `picasu-cmd` CLI. This is the only way to seed a second
/// (non-admin) user — the online bootstrap route only enrolls the first
/// admin while the store is empty — and the store's engine lock requires
/// the server to be down, so this must run pre-launch.
export async function provisionCliUsers(paths: WorkerPaths, users: CliUserDecl[]): Promise<void> {
  for (const user of users) {
    await provisionOneUser(paths, user)
  }
}

function resolveCmdBinary(): { cmd: string; args: string[]; cwd?: string } {
  const override = process.env.PICASU_CMD_BINARY
  if (override) return { cmd: path.resolve(REPO_ROOT, override), args: [] }
  for (const candidate of ['target/debug/picasu-cmd', 'target/release/picasu-cmd']) {
    if (fs.existsSync(path.resolve(REPO_ROOT, candidate))) {
      return { cmd: path.resolve(REPO_ROOT, candidate), args: [] }
    }
  }
  // Slow path: works anywhere cargo does; parallel workers serialize on the
  // build lock, correctly but slowly on first use.
  return {
    cmd: 'cargo',
    args: ['run', '-q', '-p', 'picasu', '--bin', 'picasu-cmd', '--'],
    cwd: path.resolve(REPO_ROOT, 'backend')
  }
}

function provisionOneUser(paths: WorkerPaths, user: CliUserDecl): Promise<void> {
  const { cmd, args, cwd } = resolveCmdBinary()
  const cliArgs = [
    ...args,
    'user',
    'add',
    user.user_id,
    '--password',
    user.password,
    ...(user.admin ? ['--admin'] : [])
  ]
  return new Promise((resolve, reject) => {
    const proc = spawn(cmd, cliArgs, {
      cwd,
      env: {
        ...process.env,
        PICASU_DATA_HOME: paths.DATA_DIR,
        PICASU_WEAK_PWHASH: '1'
      },
      stdio: ['ignore', 'pipe', 'pipe']
    })
    let stderr = ''
    proc.stderr?.on('data', (chunk: Buffer) => {
      stderr += chunk.toString()
    })
    proc.on('error', (err) => {
      reject(
        new Error(
          `picasu-cmd could not start (${err.message}). Build it with \`cargo build --bin picasu-cmd\` or set PICASU_CMD_BINARY.`
        )
      )
    })
    proc.on('exit', (code) => {
      if (code === 0) resolve()
      else reject(new Error(`picasu-cmd user add ${user.user_id} exited ${code}: ${stderr}`))
    })
  })
}
