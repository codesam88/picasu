/// <reference types="vite/client" />

import { z } from 'zod'

import { collectDiagnostics, getAppVersion } from '../script/utils/appInfo'

const WEB3FORMS_ENDPOINT = 'https://api.web3forms.com/submit'
const DEFAULT_ACCESS_KEY = '0c1f5526-bc2f-4f84-834c-cd8be684e3b8'
const REQUEST_TIMEOUT_MS = 15_000

export const REPORT_ISSUE_THROTTLE_KEY = 'picasu:report-issue:last-submit'
export const REPORT_ISSUE_THROTTLE_MS = 20_000

const responseSchema = z.object({
  success: z.boolean(),
  message: z.string().optional()
})

export interface SubmitReportIssueOptions {
  includeDiagnostics?: boolean
  route?: string
}

let inMemoryLastSubmit: number | null = null

function resolveAccessKey(): string {
  const envValue: unknown = import.meta.env.VITE_WEB3FORMS_KEY
  if (typeof envValue === 'string' && envValue.trim() !== '') {
    return envValue
  }
  return DEFAULT_ACCESS_KEY
}

function isLocalStorageAvailable(): boolean {
  try {
    if (typeof localStorage === 'undefined') {
      return false
    }
    const probe = '__picasu_report_issue_probe__'
    localStorage.setItem(probe, '1')
    localStorage.removeItem(probe)
    return true
  } catch {
    return false
  }
}

export function canSubmitReport(now: number = Date.now()): boolean {
  const storageAvailable = isLocalStorageAvailable()

  let last: number | null = null
  if (storageAvailable) {
    try {
      const raw = localStorage.getItem(REPORT_ISSUE_THROTTLE_KEY)
      if (raw !== null) {
        const parsed = Number(raw)
        if (Number.isFinite(parsed)) {
          last = parsed
        }
      }
    } catch {
      last = inMemoryLastSubmit
    }
  } else {
    last = inMemoryLastSubmit
  }

  if (last !== null && now - last < REPORT_ISSUE_THROTTLE_MS) {
    return false
  }

  if (storageAvailable) {
    try {
      localStorage.setItem(REPORT_ISSUE_THROTTLE_KEY, String(now))
    } catch {
      inMemoryLastSubmit = now
    }
  }
  inMemoryLastSubmit = now
  return true
}

export function buildReportMessage(
  message: string,
  route: string,
  includeDiagnostics: boolean
): string {
  const diagnostics = collectDiagnostics(route)
  const diagnosticsBlock = includeDiagnostics
    ? [
        `appVersion: ${diagnostics.appVersion}`,
        `route: ${diagnostics.route}`,
        `userAgent: ${diagnostics.userAgent}`,
        `timestamp: ${diagnostics.timestamp}`,
        `viewport: ${diagnostics.viewport}`,
        `recentErrors: ${JSON.stringify(diagnostics.recentErrors)}`
      ].join('\n')
    : '(diagnostics omitted)'
  return `${message}\n\n--- diagnostics ---\n${diagnosticsBlock}`
}

export async function submitReportIssue(
  message: string,
  routeOrOpts?: string | SubmitReportIssueOptions,
  opts?: SubmitReportIssueOptions
): Promise<void> {
  const candidates: (string | SubmitReportIssueOptions | undefined)[] = [routeOrOpts, opts]
  let route = ''
  let options: SubmitReportIssueOptions | undefined
  for (const candidate of candidates) {
    if (typeof candidate === 'string') {
      route = candidate
    } else if (candidate !== undefined) {
      options = candidate
      if (typeof candidate.route === 'string') {
        route = candidate.route
      }
    }
  }

  if (route === '' && typeof window !== 'undefined') {
    try {
      route = window.location.pathname
    } catch {
      route = ''
    }
  }

  const includeDiagnostics = options?.includeDiagnostics !== false

  const accessKey = resolveAccessKey()
  const version = getAppVersion()
  const timestamp = new Date().toISOString()

  const fullMessage = buildReportMessage(message, route, includeDiagnostics)

  const payload = {
    access_key: accessKey,
    subject: `Picasu problem report ${version} ${timestamp}`,
    message: fullMessage,
    from_name: 'Picasu in-app report',
    botcheck: ''
  }

  const controller = new AbortController()
  const timer = setTimeout(() => {
    controller.abort()
  }, REQUEST_TIMEOUT_MS)

  let response: Response
  try {
    response = await fetch(WEB3FORMS_ENDPOINT, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'application/json'
      },
      body: JSON.stringify(payload),
      signal: controller.signal
    })
  } catch (error) {
    clearTimeout(timer)
    throw error instanceof Error ? error : new Error('Report failed')
  }
  clearTimeout(timer)

  let parsedBody: unknown
  try {
    parsedBody = await response.json()
  } catch {
    parsedBody = {}
  }

  const parsed = responseSchema.safeParse(parsedBody)
  if (!response.ok) {
    const msg = parsed.success ? parsed.data.message : undefined
    throw new Error(msg ?? 'Report failed')
  }
  if (!parsed.success) {
    throw new Error('Report failed')
  }
  if (!parsed.data.success) {
    throw new Error(parsed.data.message ?? 'Report failed')
  }
}
