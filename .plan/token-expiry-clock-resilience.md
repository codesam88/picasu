---
status: idea
type: feature
priority: low
area: frontend
---

## Notes

Serve-token expiry judgment resilient to a wrong client wall clock. Idea,
not fully settled; pairs naturally with the proactive-renewal-with-buffer
gate discussed with the user — if that lands first, this refines it.

### Problem

Serving tokens (`ClaimsTimestamp`, `ClaimsHash` — both TTL 300 s,
`auth.rs`) carry a plaintext `exp` in server-frame epoch seconds, but the
frontend gate compares it against the local clock
(`tokenStore._isExpired` via `jwt-decode`). On a device with a wrong
clock this fails in two directions: ahead-clock causes redundant renewals
(harmless), behind-clock masks real expiry until the server 401s each
token once per TTL. The reactive force-renew leg makes both directions
correct, but only reactively.

### Goal

Anchor expiry to the server's clock at token receive time, so the local
gate stays correct even with a bad wall clock:

- At token receive time (initial fetch and every renewal response),
  read the HTTP `Date` response header; compute the remaining lifetime
  `lifetime = exp - serverTime` and store
  `ownExpiry = local now() + lifetime` next to the token.
- The proactive gate (`renew-if-within-buffer`) compares against
  `ownExpiry`, never against the raw `exp`, making the client's
  judgment independent of wall-clock error (up to Date-header
  second-resolution granularity).
- Server time stays the only authority on expiry: a live 401 still
  ranks above both `exp` and `ownExpiry` (force-renew; the landing
  login-banner contract is the escalation).

### Implementation sketch (when picked up)

1. Extract the `Date` header in the token-renewal/fetch response path
   (`tokenStore._updateTimestampToken` / `_updateAssetToken`, plus the
   initial token fetch); fall back to the raw `exp` comparison when the
   header is absent.
2. Store `ownExpiry` alongside the token in `tokenStore` state (scalar
   for the timestamp token, per-asset map next to `assetTokenMap`).
3. Re-anchor on wake: on resume from suspension a monotonic-style
   countdown over-trusts; recompute skew from the next response or rely
   on the 401 reactive leg (decide at design time).
4. Decide whether the proactive-with-buffer gate lands in this task or
   in a preceding one (a plain currently-expired gate + this anchor is
   also a valid pairing).

### Gate: evidence before cleverness (2026-10-18)

The complexity-risk concern is recorded as a decision gate. Landed
posture:

1. The simple proactive buffer (renew when the raw/anchored expiry is
   within n minutes, existing dispatch-time gate, no new state) is the
   only part worth landing by default — one-line threshold change,
   reactive force-renew already correct for any clock.
2. The Date-header anchor is a candidate only if field evidence shows
   wrong clocks actually occur: measure first with **shadow
   instrumentation** — compute both judgments (raw `exp` vs anchored
   `ownExpiry`) on every dispatch, log disagreements + renewals +
   401-forced renewals, flip no behavior. Implement the anchor only if
   disagreements materialize beyond the benign ahead-clock churn.
3. A/B testing of user-visible behavior is impractical here: the
   affected population (bad clocks) is tiny and the difference is
   invisible UX-wise; shadow metrics + deterministic vitest skew
   scenarios give the same answer without asking users to carry a
   change whose failure modes are the risk. Simplify-or-drop stays open
   if instrumentation shows near-zero effect.

### Known open questions

- Does every response path in the renewal chain actually carry a
  `Date` header (nginx/proxy variance)? Fallback must not silently
  degrade the gate.
- Per-asset tokenMaps: one re-anchor per renewal is fine, but whether
  per-asset renewals collapse into a shared round-trip (the promise-lock
  pattern generalized) is a separate decision.
