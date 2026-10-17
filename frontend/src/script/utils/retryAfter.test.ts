import { afterEach, describe, expect, test, vi } from 'vitest'
import { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import { retryAfterSecs, startRetryCountdown, throttleMessage } from './retryAfter'

/// Build an axios-shaped 429 error. `headers` is the wire header set,
/// `body` the parsed JSON response body.
const axios429 = (headers: Record<string, string>, body?: unknown): AxiosError => {
  const config = { url: '/post/authenticate', method: 'post' } as InternalAxiosRequestConfig
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
