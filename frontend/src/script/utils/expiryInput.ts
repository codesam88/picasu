/// `<integer><unit>` expiry parser for the invite panel: `24h`, `7d`,
/// `12m` (a 30-day month, so 12m fits the one-year bound). Surrounding and
/// inner whitespace is stripped; anything else is rejected with a
/// user-facing message. Empty input falls back to the conventional 7 days;
/// `0h` maps to backend `0` (never expires). Hours are integers, so
/// sub-hour units need no rounding policy — and can never truncate to 0.
export const DEFAULT_EXPIRY_HOURS = 168
/// Longest lifetime the server accepts, in hours (one year). Mirrors
/// backend `MAX_EXPIRY_HOURS`; the panel overwrites it with the fetched
/// bound when it builds, so this is only the pre-fetch fallback.
export const MAX_EXPIRY_HOURS = 8760

export type ExpiryParse = { ok: true; hours: number } | { ok: false; error: string }

export const parseExpiryInput = (raw: string, maxHours: number): ExpiryParse => {
  const compact = raw.replace(/\s+/g, '')
  if (compact === '') return { ok: true, hours: DEFAULT_EXPIRY_HOURS }
  const match = /^(\d+)([hHdDmM])$/.exec(compact)
  if (!match) {
    return { ok: false, error: 'Enter expiry as <number><unit>, e.g. 7d (units: h/d/m)' }
  }
  const digits = match[1] ?? ''
  const unit = (match[2] ?? '').toLowerCase()
  let perUnit: number
  if (unit === 'd') perUnit = 24
  else if (unit === 'm') perUnit = 720
  else perUnit = 1 // 'h': the grammar admits only h/d/m
  const hours = Number(digits) * perUnit
  if (!Number.isSafeInteger(hours)) return { ok: false, error: 'Expiry number is too large' }
  if (hours > maxHours) return { ok: false, error: `Over the ${maxHours}h limit` }
  return { ok: true, hours }
}
