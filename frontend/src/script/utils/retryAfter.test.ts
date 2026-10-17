import { afterEach, describe, expect, test, vi } from 'vitest'
import { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import { retryAfterSecs, startRetryCountdown, throttleMessage } from './retryAfter'
import { isAuthenticateThrottle, throttleFallbackMessage, AUTHENTICATE_URL } from './retryAfter'

/// Build an axios-shaped 429 error. `headers` is the wire header set,
/// `body` the parsed JSON response body, `url` the request endpoint.
const axios429 = (
  headers: Record<string, string>,
  body?: unknown,
  url: string = AUTHENTICATE_URL
): AxiosError => {
  const config = { url, method: 'post' } as InternalAxiosRequestConfig
  const response: AxiosResponse = {
    status: 429,
    statusText: 'Too Many Requests',
    data: body,
    headers,
    config
  }
  return new AxiosError(
    'Request failed with status code 429',
    'ERR_BAD_REQUEST',
    config,
    null,
    response
  )
}

describe('retryAfterSecs', () => {
  test('reads the Retry-After header when present', () => {
    expect(retryAfterSecs(axios429({ 'retry-after': '10' }))).toBe(10)
  })

  test('falls back to the body retryAfter field when the header is absent', () => {
    expect(retryAfterSecs(axios429({}, { kind: 'Rate Limited', retryAfter: 7 }))).toBe(7)
  })

  test('falls back to the body when the header is unparsable', () => {
    expect(retryAfterSecs(axios429({ 'retry-after': 'soon' }, { retryAfter: 4 }))).toBe(4)
  })

  test('accepts a numeric body field', () => {
    expect(retryAfterSecs(axios429({}, { retryAfter: 12 }))).toBe(12)
  })

  test('is null for garbage or absent values everywhere', () => {
    expect(retryAfterSecs(axios429({ 'retry-after': 'soon' }))).toBeNull()
    expect(retryAfterSecs(axios429({}))).toBeNull()
    expect(retryAfterSecs(axios429({}, { retryAfter: 'later' }))).toBeNull()
    expect(retryAfterSecs(axios429({}, { retryAfter: null }))).toBeNull()
  })

  test('is null for zero or negative durations', () => {
    expect(retryAfterSecs(axios429({ 'retry-after': '0' }))).toBeNull()
    expect(retryAfterSecs(axios429({ 'retry-after': '-5' }))).toBeNull()
    expect(retryAfterSecs(axios429({}, { retryAfter: 0 }))).toBeNull()
    expect(retryAfterSecs(axios429({}, { retryAfter: -3 }))).toBeNull()
  })

  test('is null for non-axios errors and network failures without a response', () => {
    expect(retryAfterSecs(new Error('boom'))).toBeNull()
    expect(retryAfterSecs(undefined)).toBeNull()
    const network = new AxiosError('Network Error', 'ERR_NETWORK')
    expect(retryAfterSecs(network)).toBeNull()
  })
})

describe('startRetryCountdown', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  test('ticks down once per second and reports done at zero', () => {
    vi.useFakeTimers()
    const ticks: number[] = []
    const onDone = vi.fn()

    startRetryCountdown(3, (remaining) => ticks.push(remaining), onDone)

    expect(ticks).toEqual([3])
    vi.advanceTimersByTime(1000)
    expect(ticks).toEqual([3, 2])
    expect(onDone).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1000)
    expect(ticks).toEqual([3, 2, 1])
    vi.advanceTimersByTime(1000)
    expect(onDone).toHaveBeenCalledTimes(1)
    expect(ticks).toEqual([3, 2, 1])
  })

  test('a fractional duration still shows a whole second', () => {
    vi.useFakeTimers()
    const ticks: number[] = []

    startRetryCountdown(0.4, (remaining) => ticks.push(remaining), vi.fn())

    expect(ticks).toEqual([1])
    vi.advanceTimersByTime(1000)
    expect(ticks).toEqual([1])
  })

  test('stop cancels the countdown without reporting done', () => {
    vi.useFakeTimers()
    const ticks: number[] = []
    const onDone = vi.fn()

    const stop = startRetryCountdown(5, (remaining) => ticks.push(remaining), onDone)
    stop()
    vi.advanceTimersByTime(10_000)

    expect(ticks).toEqual([5])
    expect(onDone).not.toHaveBeenCalled()
  })

  test('throttle message names the wait in seconds', () => {
    expect(throttleMessage(10)).toBe('Too many attempts — try again in 10s')
    expect(throttleMessage(1)).toBe('Too many attempts — try again in 1s')
  })
})

describe('isAuthenticateThrottle', () => {
  test('is true for an axios 429 on the authenticate endpoint', () => {
    expect(isAuthenticateThrottle(axios429({}, { retryAfter: 5 }))).toBe(true)
  })

  test('is true for a hint-less authenticate 429 too (no banner countdown possible)', () => {
    expect(isAuthenticateThrottle(axios429({}))).toBe(true)
  })

  test('is false for a 429 on any other route (the interceptor owns those)', () => {
    expect(isAuthenticateThrottle(axios429({}, { retryAfter: 5 }, '/get/get-data'))).toBe(false)
  })

  test('is false for non-429 statuses on the authenticate endpoint', () => {
    const config = { url: AUTHENTICATE_URL, method: 'post' } as InternalAxiosRequestConfig
    const response: AxiosResponse = {
      status: 401,
      statusText: 'Unauthorized',
      data: {},
      headers: {},
      config
    }
    expect(
      isAuthenticateThrottle(
        new AxiosError(
          'Request failed with status code 401',
          'ERR_BAD_REQUEST',
          config,
          null,
          response
        )
      )
    ).toBe(false)
  })

  test('is false for non-axios errors and responseless failures', () => {
    expect(isAuthenticateThrottle(new Error('boom'))).toBe(false)
    expect(isAuthenticateThrottle(undefined)).toBe(false)
    expect(isAuthenticateThrottle(new AxiosError('Network Error', 'ERR_NETWORK'))).toBe(false)
  })
})

describe('throttleFallbackMessage', () => {
  test('prefers the server message when the body carries one', () => {
    expect(
      throttleFallbackMessage(
        axios429({}, { kind: 'Rate Limited', message: 'Too many login attempts' })
      )
    ).toBe('Too many login attempts')
  })

  test('falls back to a generic throttle copy when the body is missing or unusable', () => {
    for (const err of [
      axios429({}),
      axios429({}, { retryAfter: 5 }),
      axios429({}, { message: '' }),
      axios429({}, 'not-an-object')
    ]) {
      const copy = throttleFallbackMessage(err)
      expect(typeof copy).toBe('string')
      expect(copy.length).toBeGreaterThan(0)
      expect(copy).not.toContain('429')
    }
  })

  test('falls back to the generic copy for non-axios errors', () => {
    expect(throttleFallbackMessage(new Error('boom')).length).toBeGreaterThan(0)
    expect(throttleFallbackMessage(undefined).length).toBeGreaterThan(0)
  })
})
