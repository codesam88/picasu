---
status: done
type: feature
priority: high
area: backend
---

## Notes

R7 (also `auth-hardening.md` F5): no rate limiting on credential paths.
Sixteen consecutive wrong passwords return 401 with no backoff/429, and
failures log without client address. PBKDF2-600k raises per-guess cost
only. Goal: log client IP on auth failure + three throttle buckets
(IP-short burst, IP-long daily per source, user-long daily per account)
returning 429 with `Retry-After`, checked before the KDF so throttled
requests stay cheap. Locked 2026-10-17.

## Decisions (settled 2026-10-16 interview)

- Scope slice 1: `POST /post/authenticate` only. Bootstrap/register/
  password-change extend later (same limiter, separate step).
- IP source: configurable trust. New knob `trust_proxy_forwarded_for: bool`
  under `[server]`, default false. `false` = Rocket `client_ip()` socket
  addr only, `X-Forwarded-For` ignored. `true` = leftmost
  `X-Forwarded-For` entry when present and parseable, else socket addr.
  Default LAN deploy must not trust a spoofable header; reverse-proxy
  deploy opts in.
- Throttled answer: 429 + `Retry-After: <secs>` + JSON body, distinct
  from 401 bad-creds. Checked pre-KDF so throttled requests stay cheap.
- Tunability: config knobs under `[server]` (TOML + env), not hardcoded:
  `login_ip_window_secs` (600), `login_ip_max_failures` (3),
  `login_ip_base_ban_secs` (10), `login_ip_max_ban_secs` (28800),
  `login_ip_long_window_secs` (86400), `login_ip_long_max_failures` (16),
  `login_ip_long_base_ban_secs` (1800), `login_ip_long_max_ban_secs` (28800),
  `login_user_long_window_secs` (86400), `login_user_long_max_failures` (18),
  `login_user_long_base_ban_secs` (1800), `login_user_long_max_ban_secs` (7200),
  plus `trust_proxy_forwarded_for` (false).
  Kill switch `PICASU_RATE_LIMITS=false` (env-only, like
  `PICASU_PBKDF_ITERATIONS`: never file, never API) bypasses all
  throttling for schemathesis fuzzing; failure logging stays. Only the
  exact string `false` disables; anything else means enabled. Warn-logs
  once when disabled.
  Defaults = oracle tables below. Buckets locked 2026-10-17: IP-short +
  IP-long + user-long (user-short dropped; no per-(IP,user) pair bucket).
- Success drops the username entry and clears the IP burst bucket but
  keeps the IP-long history (fix batch 2026-10-17: otherwise any valid
  account holder could wipe a shared IP's daily budget with one login);
  failures increment all applicable buckets (IP-short, IP-long, user-long).
- User buckets are keyed on the claimed (validated, trimmed) username whether or not the account exists. Unknown names get the same 401 + throttle behavior as known ones (no existence oracle); enumeration guesses burn the same budgets. Invalid IDs (400) never enter the buckets. Entries are pruned when their window expires so random-name spraying cannot grow state without bound.
- State: in-memory `DashMap` (already a dependency), reset on restart.
- `Bucket::new` rejects degenerate config (zero window/max/base, cap <
  base) instead of clamping — the `[server]` plumbing (step 4) must
  surface these as config load errors. `ban_delay` saturates at the cap
  (pinned by overflow tests) so absurd-but-valid numbers can't wrap or hang.

## Delay oracle (proposed unit-test table)

Semantics: sliding window counts failures; once `max_failures`
in-window is exceeded, the next attempt is 429 with
`Retry-After = ban(n)`, where `ban(n) = min(base * 2^n, cap)` and two
evaluated over-threshold failures share each generation: with `count` =
in-window failure count, `n = (count - max - 1) / 2` (integer division),
so over-threshold failures #1–2 ban `base`, #3–4 ban `2*base`, and so on
up to `cap`. A throttled (blocked) attempt is never recorded, so it
neither extends nor escalates the ban (stable countdown). Success clears
the bucket. Generation decays automatically: timestamps aging out of the
window lower `count` and hence `n` with no separate timer. Purge rule:
state disappears after `max(ban_until, last failure + window)` of
silence — the window and the ban count down in parallel.

IP-short: initial small delay (3/10 min → 10×2, 20×2, 40×2, 80×2, 160×2, …, 8h cap):

| over-threshold # | overall failure # | ban             |
| ---------------- | ----------------- | --------------- |
| 1–2              | 4th–5th           | 10s             |
| 3–4              | 6th–7th           | 20s             |
| 5–6              | 8th–9th           | 40s             |
| 7–8              | 10th–11th         | 80s             |
| 9–10             | 12th–13th         | 160s            |
| 11–12            | 14th–15th         | 320s            |
| 13–14            | 16th–17th         | 640s            |
| 15–16            | 18th–19th         | 1280s           |
| 17–18            | 20th–21st         | 2560s           |
| 19–20            | 22nd–23rd         | 5120s           |
| 21–22            | 24th–25th         | 10240s          |
| 23–24            | 26th–27th         | 20480s          |
| 25+              | 28th+             | 28800s (8h cap) |

IP-long: per-source daily ceiling (16/24h → 30m×2, 60m×2, 120m×2, …, 8h cap).
Catches paced spraying across many usernames that stays under the burst
bucket; one egress IP gets 16 free guesses a day however it spreads them:

| over-threshold # | overall failure # | ban             |
| ---------------- | ----------------- | --------------- |
| 1–2              | 17th–18th         | 1800s (30m)     |
| 3–4              | 19th–20th         | 3600s (60m)     |
| 5–6              | 21st–22nd         | 7200s (120m)    |
| 7–8              | 23rd–24th         | 14400s (4h)     |
| 9+               | 25th+             | 28800s (8h cap) |

User-long: per-account daily ceiling (18/24h → 30m×2, 60m×2, 2h cap).
The botnet backstop: many IPs hammering one account still share this
budget, with a slightly higher trigger than the IP-long bucket so
IP-based filters fire first. Capped at 2h (not 8h) so a real owner
caught in background noise can always retry within hours:

| over-threshold # | overall failure # | ban            |
| ---------------- | ----------------- | -------------- |
| 1–2              | 19th–20th         | 1800s (30m)    |
| 3–4              | 21st–22nd         | 3600s (60m)    |
| 5+               | 23rd+             | 7200s (2h cap) |

Worked example (IP-short): 3x 401 in 10 min -> 4th attempt within
window = 429 `Retry-After: 10`; post-expiry failure shares the rung
(10s), the next pair steps to 20s. Sustained best rates: stay-clean
432/day per IP (short), 16/day per IP and 18/day per account (longs);
ban-eating decays to ~6/day (8h caps) / 24/day (2h cap). First-day haul
≈ 16–18 free guesses — top-18 passwords against a known name stay the
residual risk,
carried by the password policy, not the buckets.

## 429 wire format + login display (settled 2026-10-16)

- Backend: `429 Too Many Requests` with standard `Retry-After: <secs>`
  header where `<secs>` is the max remaining ban across IP-short,
  IP-long, and user-long (whichever bucket blocks longer wins; all
  buckets are still recorded independently). JSON body carries the same
  seconds in a machine field plus a human message, e.g.
  `{ "kind": "Rate Limited", "message": "Too many login attempts, try again in 10s", "retryAfter": 10 }`.
  Requires a new `ErrorKind::RateLimited -> 429` plus a `retry_after`
  carrier so the `Responder` can set the header (current `AppError`
  sets status only, no header).
- Pre-KDF check order per attempt: compute remaining bans for
  IP-short, IP-long, and user-long (user keyed on the claimed name even
  when unknown); if any > 0, return 429 immediately (no KDF, no counter
  increment, but DO log `ip=` + `user_id=` at warn). Else run KDF:
  success resets the caller's IP + user buckets; failure records into
  all three buckets and returns 401 — unless the recording itself opens
  a new ban, in which case still return 401 this time and 429 from the
  next attempt (no same-request escalation surprise).
- Frontend `LoginPage`: on 429, show inline banner
  `Too many attempts — try again in Xs` with a countdown, keep the
  credentials, disable the Login button until expiry, then re-enable.
  The axios interceptor gets an explicit `429` case: no session clear,
  no redirect-to-login, no generic toast for the login route (LoginPage
  owns the message; avoids the R24 double-toast).

## Rejected alternatives

- `tower-governor` / external rate-limit crate: generic per-route token
  bucket, no dual IP+user keying with exponential bans; bespoke ~150-line
  module fits better.
- Persistent (redb) counters: restart-clear is acceptable for online
  guessing defense; persistence adds write amplification on every failure.
- Captcha extension: heavier UX/API change for later if needed.

## Implementation steps (TDD red-first, one slice per commit)

1. Limiter core (`auth/rate_limit.rs`): pure sliding-window + exponential
   ban unit tests with fake clock (no Rocket dependency). Oracle table
   above becomes `ban_delay` unit tests first.
2. IP extraction + failure logging: `client_ip` helper honoring
   `trust_proxy_forwarded_for` + `warn!` on auth failure includes `ip=` +
   `user_id=`; handler tests assert log shape (or observable header in
   tests where logger capture is awkward).
3. `POST /post/authenticate` wiring: pre-KDF 429 check, post-failure
   increment, success reset; integration tests (throttle trips -> 429 +
   `Retry-After`; throttled path skips KDF; success resets).
4. Config knobs (`[server]` keys above + env) + docs (`docs/auth.md`
   throttling paragraph + `docs/config.md`) + full gates (`cargo test`,
   `just check`).
5. Tune + record accepted risks (restart-clear, single-node only,
   proxy-header trust) in plan + review file.
6. Login UI (`LoginPage` 429 banner + countdown + disabled button,
   interceptor 429 case) with vitest: 429 shows inline message, button
   disables until expiry, interceptor neither clears session nor
   redirects on 429.
7. E2E `login-throttle.spec.ts`: enroll admin, log out, 4x wrong password
   (UI, all 401) then a 5th attempt showing the first IP-short 429, with
   small `PICASU_LOGIN_IP_*` env overrides via `extraEnv` (backend
   already runs at `PICASU_PBKDF_ITERATIONS=1000`); assert the displayed
   delay, the disabled Login button, then — after the ban lapses — a
   successful login to /timeline. All attempts use the same reduced
   KDF effort; no production-iteration logins in tests.
