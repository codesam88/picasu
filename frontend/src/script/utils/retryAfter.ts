import axios from 'axios'
import { serverErrorSchema } from '@/type/schemas'

/// Login endpoint throttled by the login limiter. The response interceptor
/// stays silent for 429s here by design (the calling page owns the display
/// inline), so callers without an inline banner must display them locally.
export const AUTHENTICATE_URL = '/post/authenticate'

/// Generic copy for a throttled request with no known wait: no countdown
/// is possible, so no seconds are named.
export const GENERIC_THROTTLE_MESSAGE = 'Too many attempts — try again later'

/// Seconds until a throttled request may be retried: the `Retry-After`
/// response header first, the JSON body's `retryAfter` field as fallback.
/// Null for anything unusable (absent, non-numeric, zero, negative) —
/// callers treat that as "no known ban", never as a countdown.
export function retryAfterSecs(error: unknown): number | null {
  if (!axios.isAxiosError(error)) return null
  return (
    parseSecs(error.response?.headers?.['retry-after']) ??
    parseSecs(error.response?.data?.retryAfter)
  )
}

function parseSecs(value: unknown): number | null {
  const secs = typeof value === 'string' ? Number(value.trim()) : value
  if (typeof secs !== 'number' || !Number.isFinite(secs) || secs <= 0) return null
  return Math.ceil(secs)
}

/// Shared throttling message for the interceptor toast and the login
/// banner, so the copy cannot drift between the two sites.
export function throttleMessage(secs: number): string {
  return `Too many attempts — try again in ${secs}s`
}

/// True for a rate-limited login request: an axios 429 on the authenticate
/// endpoint, with or without a parsable retry hint. The interceptor never
/// toasts these; the caller owns the display.
export function isAuthenticateThrottle(error: unknown): boolean {
  if (!axios.isAxiosError(error)) return false
  return error.response?.status === 429 && error.config?.url === AUTHENTICATE_URL
}

/// Fallback copy for a hint-less authenticate 429: the server message when
/// the body carries one, else the generic throttle sentence. Never empty,
/// never a raw status code.
export function throttleFallbackMessage(error: unknown): string {
  if (axios.isAxiosError(error)) {
    const parsed = serverErrorSchema.safeParse(error.response?.data)
    if (parsed.success) {
      const serverMsg = parsed.data.message ?? parsed.data.error
      if (serverMsg != null && serverMsg.length > 0) return serverMsg
    }
  }
  return GENERIC_THROTTLE_MESSAGE
}

/// Drive a per-second countdown: `onTick` receives the remaining whole
/// seconds at start and after every tick, `onDone` fires once when the
/// count reaches zero. The returned stop function cancels the countdown
/// without calling `onDone`.
export function startRetryCountdown(
  totalSecs: number,
  onTick: (remainingSecs: number) => void,
  onDone: () => void
): () => void {
  let remaining = Math.max(1, Math.ceil(totalSecs))
  onTick(remaining)
  const timer = setInterval(() => {
    remaining -= 1
    if (remaining <= 0) {
      clearInterval(timer)
      onDone()
    } else {
      onTick(remaining)
    }
  }, 1000)
  return () => clearInterval(timer)
}
