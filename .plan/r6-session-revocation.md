---
status: open
type: feature
priority: high
area: backend
---

## Notes

R6: no server-side session revocation — a copied JWT stays valid to
expiry; `POST /post/logout` (added for R5) only clears the cookie.
Fix with server-side session records: each login mints a random `sid`
carried as a required claim; guards require the `sid` to be live for
that user. Logout removes only the presented `sid` (local); password
change wipes all `sid`s (global). Pre-`sid` tokens lack the claim and
die on upgrade (hard cut, no rolling compat).

Decisions (2026-10-15 interview): per-session scope (logout kills only
its own device); logout local, password change global; ignore upgrade
path; option 1 over caps (Google-style: uncapped sessions + inventory +
per-session revoke, no silent eviction). DoS handled by TTL + lazy prune
plus login cost, not by a cap — see Strategy.

## Strategy

Uncapped `jti` allowlist per user plus a session inventory (option 1:
Google-style, no silent eviction). Every login mints a random `jti`
carried as the standard registered claim; guards require the `jti` to
be live for that user. `GET /get/sessions` lists live sessions,
`POST /post/logout` (or a per-`jti` revoke) removes one, password
change wipes all. Rejected: capped allowlist (silent 9th-login evict
with no UI to manage it — the one pattern major services never ship);
per-user epoch (logout would kill every device, against the decided
scope); single active session (one device per user); short-lived access
plus refresh flow (standard but triple the change).

DoS without a cap: each record dies with its token (14-day TTL) and is
pruned lazily whenever the record loads — no sweeper, no growth beyond
churn × TTL. Churn itself costs a full PBKDF2 login per entry (plus
R7 throttling when it lands), so scripting millions of live sessions
costs the attacker far more than the defender's one small row each.
No `last-seen` writes on the hot path: records carry `jti` +
`issued-at` only, updated on login/logout, never per request (redb is
single-writer; per-request write txns would serialize all traffic).

This bends `docs/auth.md` goal 3 (no server-side session state) by one
bounded field per user; crash-restart and stateless-proxy behavior are
unchanged (records live in redb, not memory).

## Steps

1. Records (TDD, `backend/src/auth/users.rs`): done 2026-10-15
   (commit `1efe3b03e`: `SessionRecord`, `add/remove/clear_session`,
   TTL prune, read-modify-write admin paths; 26 users tests, 617 lib).
2. Token + guards (TDD): `Claims.jti` required (no default — the standard
   name, not a custom `sid`); `Claims::new_user` mints a fresh `jti`
   (uuid v4); login stores it; `try_jwt_cookie_auth` and the `GuardUser`
   path require `jti` membership; empty-store open branch skips the
   check. Done: updated helpers green; new tests: token without `jti`
   rejected, unknown `jti` rejected.
3. Rotation + inventory (TDD): logout decodes the presented cookie token
   and removes its `jti` (invalid/absent token: cookie-clear only, still
   204); `GET /get/sessions` (GuardUser: any authenticated user lists
   only their own) returns live `jti`s + `issued-at`; password-write
   paths (`PUT /put/users/password` self + admin reset) wipe all `jti`s
   — sessions first, password second, so a crash between fails safe to
   re-login-with-old-password. Done: tests (logout kills only its
   session; second device survives; password change kills all; inventory
   lists exactly live sessions), full lib suite green. Committed as
   `779a150a6` (step-3 worker transcript in session; senior review added
   let-chain `logout` cleanup + `GuardUser` presented-claims fix).
4. Docs + verify: `docs/auth.md` mechanism done in step 3. Remaining:
   Playwright first-admin live rerun (login/logout paths changed since
   the R5 run), then mark done. Explicitly out: `api_config.rs` (R2).
