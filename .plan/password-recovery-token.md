---
status: idea
type: feature
priority: medium
area: backend
---

## Notes

Problem: password recovery today means an admin directly sets a new password
via `PUT /put/users/password` (no old password needed). There is no
self-service path for a user who forgot theirs and no scoped credential the
bearer can redeem alone.

Solution strategy: single-purpose guard token minted in admin user management
that authorizes exactly one password reset for one named user (short expiry,
single use, audited). Longer term the mint may key off a per-user recovery
email address (new user attribute + verification), but email is not in the
user record today, so the first cut is admin-minted only. Rejected: emailed
reset links without a token model (needs mailer + address verification first);
reusing the session JWT as reset bearer (wrong audience and lifetime).

## Implementation steps

1. Token model (scope: `backend/src/auth/`, new recovery module): mint/verify,
   single-use store, expiry, audience binding to target `userId`. Leaves out:
   email attribute, UI.
2. Reset endpoint (scope: `backend/src/router/`): redeem token for one
   password write via `set_password_sync`, reusing policy and session-clear
   semantics. Leaves out: admin panel wiring.
3. Admin mint + optional email (scope: user management routes, user record):
   admin-only mint endpoint; recovery-email attribute deferred behind its own
   verification step. Leaves out: mailer integration.
