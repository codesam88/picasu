---
status: done
type: feature
priority: medium
area: backend
---

# User delete (admin remediation)

## Problem

A bad grant has no remediation path (auth-flows row 11). Demoting
leaves a login-capable account holding the user id — and with
invites this bites concretely: a wrongly redeemed invite squats the
id, so the intended person can never register it (409 on the name,
their invite surviving but useless). The admin needs delete.

## Strategy

Small backend + UI slice following the established route patterns:

- `DELETE /delete/users/{user_id}` (`GuardAuth` + read-only
  guard): removes the redb `USERS` record and the `passwd.json`
  entry, in that order (a crash between halves leaves a hash
  without a record — login already fails safe on unknown users via
  the dummy-KDF path). Refuses with 409 when the delete would leave
  zero admins — the same invariant as demote (`set_admin_role`
  precedent). Deleting the sole admin is therefore impossible; with
  other admins present, self-delete is allowed behind the UI
  confirm dialog.
- 404 unknown id, 400 invalid id, full `#[utoipa::path]`
  annotations, openapi regen.
- UI: delete button next to each non-locked row in the user panel
  (reuse the demote confirm-dialog pattern), verbatim server
  errors, list refresh.
- Tests: route matrix (delete ok, unknown 404, last-admin 409,
  non-admin 401, read-only 405, passwd entry gone, login after
  delete 401s); frontend vitest for the panel wiring.

## Steps

1. Backend route + helper + matrix (TDD), openapi regen + gates.
2. Panel UI + vitest.
3. Docs line in `docs/auth.md` (user lifecycle), gates green.

## Leaves out

- Disable/suspend states (delete covers remediation).
- User rename (no route; the squat case is resolved by delete +
  re-register).
- Bulk delete (one id per request, like the rest of the API).

## Outcome (2026-10-27)

Shipped as `DELETE /delete/users/{user_id}` (`backend/src/router/delete.rs`):
`GuardAuth` + `GuardReadOnlyMode` (405 while read-only), 400 invalid id,
404 unknown, 409 zero-admin, 204 success. The two-store premise of the
strategy aged out — the identity record and the password hash live in one
redb row (`users.redb`), so removal is a single store write; the
zero-admin invariant it required survives inside that transaction via
`user_store::delete` (the same `Conflict` path `set_admin_role` uses).
Panel UI: delete button beside the demote control in `UserManagement.vue`,
hidden on the locked sole-admin row, confirm dialog + verbatim server
error + list refresh. Pinned by ten named route tests (store entry gone,
login refuses afterwards, self-delete with another admin present) plus
vitest wiring tests.
