---
status: open
type: feature
priority: medium
area: frontend
---

## Notes

Simplify worker-token 401 handling to three moves: renew-in-advance,
report-once, converge-on-main. Backend needs no change — the renewal
endpoints already renew any correctly signed serving token regardless of
expiry (`VALIDATION_ALLOW_EXPIRED` in `renew_timestamp_token`,
`auth.rs`), keeping the snapshot timestamp claim; TTL is a hardcoded
300 s on both serving tokens (`ClaimsTimestamp` / `ClaimsHash`).

## Problem

Worker-realm 401s currently mean "log out" (`unauthorized` message →
clear profile + redirect, T4 in `r24-r27-throttling-cleanup.md`). A 401
on a token handed to a worker can, however, be a stale-token event with
a perfectly alive session — pre-dispatch gating only checks `exp < now`
with zero margin against a 300 s TTL, so the dispatch→server window can
cross the deadline. Escalating to logout there is too aggressive, and
the declared `refresh*` back-channels remain unwired.

## Goal

1. **Renew-in-advance** (main thread, pre-dispatch): renew whenever a
   token would be handed to a worker with less than `RENEW_BUFFER_SECS`
   of validity left. One-line gate widening; the existing promise-lock
   single-flights stays; no timers; nothing renews while nothing
   dispatches.
2. **Report-once** (worker realm): a 401 is emitted once and rejected.
   The data worker reports "rejected"; the image worker reports
   "rejected" with the asset it was fetching (the assetId only exists in
   its catch block). No retry counts, queues, or policy in either realm.
3. **Converge-on-main** (single handling point): the main-side handler
   force-renews server-authoritatively (unconditional — the local exp
   check is exactly what just failed) and logs. Recovery is ambient: a
   401'd batch never set `batchFetched`, so the next scroll trigger
   refills it; re-queued images re-read the updated `assetTokenMap`.
4. **Escalation as a consequence**: if the session is actually dead, the
   force-renew call itself 401s on main-thread axios and the standard
   interceptor contract owns it (clear profile → one redirect → the
   login-page banner). The main-side handler contains no redirect logic;
   the T4-era clearProfile+redirect in the `unauthorized` handlers is
   replaced by this.

## Settled decisions

2026-10-18, by user review:

- Buffer `RENEW_BUFFER_SECS = 120` (TTL 300 s → active-browsing renewal
  cadence ≤1 per ~3 min; skip config knob for a frontend constant).
- The image worker reports too, with `assetId` — silent image-401s would
  re-read the stale token on every reflow; the report makes recovery
  deterministic.
- The message is renamed `unauthorized` → `tokenExpired` (its meaning is
  "the token you gave me was rejected", not "log out").
- The clock-anchoring scheme stays idea-level (`token-expiry-clock-
resilience.md`); the plain buffer gate composes with it independently.

## Implementation steps

1. **Verify-first (blocker for 3):** map the share-path renewal
   rejections: which HTTP status do `renew-timestamp-token` /
   `renew-hash-token` give when the embedded share was disabled or
   expired (`ErrorKind::Auth` mapping), and does the main axios
   interceptor's share-page gate route those to the share flow instead
   of the login redirect? The handler must not escalate a revoked share
   to a login redirect. Also confirm the escalation path: session-dead
   renewal → 401 → `handleGeneralError` (clear profile + single
   redirect + banner).
2. `tokenStore`: add `RENEW_BUFFER_SECS = 120`; widen both gates
   (`refreshTimestampTokenIfExpired`, `_ensureAssetTokenFresh`) to
   "within buffer"; add unconditional `forceRenewTimestampToken()` /
   `forceRenewAssetToken(assetId)` reusing the update helpers and the
   renewal lock. Vitest with skew scenarios (`vi.setSystemTime` ahead /
   behind the server clock), buffer-boundary pin, force-renew
   unconditional pin.
3. Wire the report: `workerApi.ts` rename `unauthorized` →
   `tokenExpired`; data-worker payload empty, image-worker payload
   carries `assetId`. Data worker: interceptor
   `onUnauthorized → postToMainData.tokenExpired()` (its fetches have no
   catch — the interceptor branch stays the single 401 point). Image
   worker: report from the existing catch block
   (`axios.isAxiosError && status === 401 → postToMainImg.tokenExpired
({assetId})`) since `event.assetId` is visible only there; its
   interceptor keeps the info-level log. Main handlers:
   `tokenExpired` → force-renew (+ `console.info`), no redirect inside
   the handler.

4. Vitest for the wiring per repo conventions (behavior tests where the
   unit is testable; structure pins where a realm boundary prevents it),
   full frontend suite + `just frontend-check` green.
5. Docs: `docs/auth.md` — replace/extend the session-denial section with
   the renewal-first contract (worker 401 → force-renew → ambient
   retry; escalation only via the renewal's own 401); cross-ref the
   superseded T4 handler text. `just check` green before done.

## Known open questions

- Image-worker 401 produces two logs (interceptor info + catch
  console.error) — acceptable noise unless implementation shows a
  cheaper single-point shape.
- `Date`-header anchoring (see `token-expiry-clock-resilience.md`)
  remains out of scope here.
