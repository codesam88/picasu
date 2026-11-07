---
status: idea
type: feature
priority: low
area: backend
---

## Notes

Extend `GuardReauth` from password-per-action into a proper short-lived
re-auth ticket proving the client authenticated recently (minutes), not
just that it holds a session token.

## Current state

`GuardReauth` (`backend/src/router/auth.rs`) requires the session JWT
plus the `X-Picasu-Password` header on every sensitive request
(users_admin, users_password, admin_config, user delete). The password
is re-verified (throttled) per request; missing/wrong → 403. Correct
but chatty: each sensitive action resends the password, and multi-step
admin flows re-prompt per action.

## Proposal

- After one successful password verification, the server issues a
  re-auth ticket: short expiry (a few minutes), bound to the session
  (`jti`) and possibly the action class, presented instead of the
  password on subsequent sensitive requests.
- Frontend caches the ticket in memory (never persisted), attaches it
  like today, and re-prompts for the password only when the ticket is
  absent or expired. Expiry UX: the 403 path already exists — reuse it
  to trigger the password step.
- Ticket issuance stays behind the same throttle as password verify;
  password change and logout revoke outstanding tickets.

## Scope (when picked up)

1. Backend: ticket mint/verify (format, expiry, session binding,
   revocation on password change/logout), `GuardReauth` accepts ticket
   or password, tests for expiry/binding/revocation.
2. Frontend: in-memory ticket cache, attach-to-request, expiry →
   password-step UX in the two-step confirm flows.
3. Docs: auth-matrix/OpenAPI updates for the new scheme.

## Open questions

- Ticket format: signed JWT-like token vs opaque server-side record.
- Expiry length and whether it slides on use.
- Single-use vs multi-use within the window; replay/binding needs.
- Whether issuance needs its own throttle bucket distinct from login.

## Relations

- Builds on the `GuardReauth` rollout (users_admin re-auth).
- Complements `auth-hardening`; orthogonal to the activity-log plans.
