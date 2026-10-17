import axios from 'axios'

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
