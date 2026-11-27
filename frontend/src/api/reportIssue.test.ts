import { describe, expect, test, vi, beforeEach, afterEach } from 'vitest'
import {
  submitReportIssue,
  canSubmitReport,
  buildReportMessage,
  REPORT_ISSUE_THROTTLE_KEY,
  REPORT_ISSUE_THROTTLE_MS
} from './reportIssue'

const ENDPOINT = 'https://api.web3forms.com/submit'
const DEFAULT_KEY = '0c1f5526-bc2f-4f84-834c-cd8be684e3b8'

function mockFetch(response: {
  ok?: boolean
  status?: number
  body?: unknown
}): ReturnType<typeof vi.fn> {
  const fn = vi.fn().mockResolvedValue({
    ok: response.ok ?? true,
    status: response.status ?? 200,
    json: () => Promise.resolve(response.body ?? {})
  })
  vi.stubGlobal('fetch', fn)
  return fn
}

beforeEach(() => {
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('submitReportIssue', () => {
  test('POSTs to the Web3Forms endpoint with JSON headers and required payload fields', async () => {
    const fetchMock = mockFetch({ body: { success: true } })
    await submitReportIssue('hello', '/route')

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const firstCall = fetchMock.mock.calls.at(0)
    if (!firstCall) throw new Error('expected a fetch call')
    const [url, init] = firstCall as [string, RequestInit]
    expect(url).toBe(ENDPOINT)
    expect(init.method).toBe('POST')
    expect(init.headers).toEqual({
      'Content-Type': 'application/json',
      Accept: 'application/json'
    })

    const body = JSON.parse(init.body as string) as Record<string, unknown>
    expect(body.access_key).toBe(DEFAULT_KEY)
    expect(body.subject).toMatch(/^Picasu problem report 2\.1\.0 .+$/)
    expect(body.from_name).toBe('Picasu in-app report')
    expect(body.botcheck).toBe('')
    expect(body.message).toContain('hello')
  })

  test('includes diagnostics with version and route when enabled', async () => {
    const fetchMock = mockFetch({ body: { success: true } })
    await submitReportIssue('hello', '/route', { includeDiagnostics: true })

    const call = fetchMock.mock.calls.at(0)
    if (!call) throw new Error('expected a fetch call')
    const init = call[1] as RequestInit
    const body = JSON.parse(init.body as string) as { message: string }
    expect(body.message).toContain('2.1.0')
    expect(body.message).toContain('/route')
    expect(body.message).not.toContain('(diagnostics omitted)')
  })

  test('uses the omitted note when diagnostics are toggled off', async () => {
    const fetchMock = mockFetch({ body: { success: true } })
    await submitReportIssue('hello', '/route', { includeDiagnostics: false })

    const call = fetchMock.mock.calls.at(0)
    if (!call) throw new Error('expected a fetch call')
    const init = call[1] as RequestInit
    const body = JSON.parse(init.body as string) as { message: string }
    expect(body.message).toContain('(diagnostics omitted)')
  })

  test('rejects when the response reports success: false', async () => {
    mockFetch({ body: { success: false, message: 'nope' } })
    await expect(submitReportIssue('hi', '/')).rejects.toThrow('nope')
  })

  test('rejects on a non-2xx response', async () => {
    mockFetch({ ok: false, status: 500, body: {} })
    await expect(submitReportIssue('hi', '/')).rejects.toThrow()
  })

  test('rejects when the request is aborted on timeout', async () => {
    vi.useFakeTimers()
    const fetchMock = vi.fn().mockImplementation((_url: string, init: RequestInit) => {
      return new Promise((_resolve, reject) => {
        if (init.signal) {
          init.signal.addEventListener('abort', () => {
            reject(new DOMException('Aborted', 'AbortError'))
          })
        }
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const promise = submitReportIssue('hi', '/')
    vi.advanceTimersByTime(15_000)
    await expect(promise).rejects.toThrow()
  })

  test('uses the env key when VITE_WEB3FORMS_KEY is set', async () => {
    vi.stubEnv('VITE_WEB3FORMS_KEY', 'env-key-123')
    const fetchMock = mockFetch({ body: { success: true } })
    await submitReportIssue('hi', '/')

    const call = fetchMock.mock.calls.at(0)
    if (!call) throw new Error('expected a fetch call')
    const init = call[1] as RequestInit
    const body = JSON.parse(init.body as string) as { access_key: string }
    expect(body.access_key).toBe('env-key-123')
  })

  test('falls back to the default key when the env key is empty', async () => {
    vi.stubEnv('VITE_WEB3FORMS_KEY', '')
    const fetchMock = mockFetch({ body: { success: true } })
    await submitReportIssue('hi', '/')

    const call = fetchMock.mock.calls.at(0)
    if (!call) throw new Error('expected a fetch call')
    const init = call[1] as RequestInit
    const body = JSON.parse(init.body as string) as { access_key: string }
    expect(body.access_key).toBe(DEFAULT_KEY)
  })
})

describe('canSubmitReport', () => {
  test('records and throttles within the storage-backed session', () => {
    const backing = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => backing.get(key) ?? null,
      setItem: (key: string, value: string) => {
        backing.set(key, value)
      },
      removeItem: (key: string) => {
        backing.delete(key)
      }
    })

    const base = 1_000_000
    expect(canSubmitReport(base)).toBe(true)
    expect(backing.get(REPORT_ISSUE_THROTTLE_KEY)).toBe(String(base))
    expect(canSubmitReport(base + 5_000)).toBe(false)
    expect(canSubmitReport(base + 25_000)).toBe(true)
  })

  test('throttles in-memory when localStorage is unavailable', () => {
    vi.unstubAllGlobals()
    const base = 2_000_000
    expect(canSubmitReport(base)).toBe(true)
    expect(canSubmitReport(base + 5_000)).toBe(false)
    expect(canSubmitReport(base + 25_000)).toBe(true)
  })

  test('does not throw when localStorage is unavailable', () => {
    vi.unstubAllGlobals()
    expect(() => canSubmitReport()).not.toThrow()
  })

  test('exports the throttle key and interval', () => {
    expect(REPORT_ISSUE_THROTTLE_KEY).toBe('picasu:report-issue:last-submit')
    expect(REPORT_ISSUE_THROTTLE_MS).toBe(20_000)
  })
})

describe('buildReportMessage', () => {
  test('includes diagnostics when enabled', () => {
    const result = buildReportMessage('hello', '/route', true)
    expect(result).toContain('hello')
    expect(result).toContain('--- diagnostics ---')
    expect(result).not.toContain('(diagnostics omitted)')
    expect(result).toContain('/route')
  })

  test('omits diagnostics when disabled', () => {
    const result = buildReportMessage('hello', '/route', false)
    expect(result).toContain('hello')
    expect(result).toContain('(diagnostics omitted)')
  })
})
