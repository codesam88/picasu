// App version source of truth is frontend/package.json ("version").
import { getRecentErrors } from '../utils/errorLog'

export const APP_VERSION = '2.1.0'

export function getAppVersion(): string {
  return APP_VERSION
}

export interface Diagnostics {
  appVersion: string
  route: string
  userAgent: string
  timestamp: string
  viewport: string
  recentErrors: string[]
}

export function collectDiagnostics(route: string): Diagnostics {
  let width: number | undefined
  let height: number | undefined
  if (typeof window !== 'undefined') {
    const w = window as { innerWidth?: number; innerHeight?: number }
    width = w.innerWidth
    height = w.innerHeight
  }
  if (width === undefined && typeof globalThis.innerWidth !== 'undefined') {
    width = globalThis.innerWidth
    height = globalThis.innerHeight
  }
  const viewport = width !== undefined && height !== undefined ? `${width}x${height}` : '0x0'

  const userAgent = typeof navigator !== 'undefined' ? navigator.userAgent : ''

  return {
    appVersion: APP_VERSION,
    route,
    userAgent,
    timestamp: new Date().toISOString(),
    viewport,
    recentErrors: getRecentErrors()
  }
}
