---
status: in-progress
type: feature
priority: high
area: backend
---

## Notes

Admin interface to grant/revoke the admin role per user. A user-settings
panel in the frontend next to backend settings (a new block on `ConfigPage`),
visible only to users with the admin role.

### Bounded change

- Outcome: an admin can list users and toggle each user's admin flag from
  the Settings page; non-admins never see the panel; the server enforces
  every rule regardless of UI.
- In scope: `PUT /put/users/admin` (+ backend tests); `src/api/users.ts`;
  current-user role state (`authStore`, decoded from the readable `jwt`
  cookie with the existing `jwt-decode` dep); `UserManagement.vue` panel
  block on `ConfigPage` gated on admin; login page user-id field (without
  it nobody can log in once users exist — the bare-string body only works
  on the empty-store legacy path); `openapi.json`; vitest for the pure
  decode helper; small `docs/auth.md` touch.
- Out: create-user UI, password-management UI (`ChangePassword.vue` keeps
  working — it already targets the caller's own password), album ACL,
  per-user settings values.

### Decisions

- Roles stay boolean (`admin: true/false`); no new roles.
- Server-side safety, all enforced in the handler, never UI-only. One
  unified rule: **a change that would leave zero admins is refused** (409).
  This covers self-demotion as sole admin and demoting the sole other admin
  alike, and it enables adminship transfer (self-demotion with other admins
  present is allowed). No-op writes → 200. Unknown user → 404. Non-admin /
  unauthenticated / bad id → 401 / 401 / 400. Share credentials → 401.
- The zero-admin check and the write happen in **one redb write transaction**
  (atomic by construction — the race is review-only, not tested).
- Route carries `GuardAuth` (admin-only, caller id from its claims) +
  `GuardReadOnlyMode` like other mutators.
- Every grant/revocation writes one structured server-log line
  (actor, target, new value). No re-authentication for privilege change:
  accepted risk, consistent with the readable-cookie session posture.
- Frontend role source: decode the `jwt` cookie **lazily in a computed**
  (always fresh, no hydration wiring to rot) with the existing `jwt-decode`
  dep. No new endpoint, no new deps.
- UI predicates (`canSeePanel`, self-row state, error-to-message) are pure
  exported helpers with vitest coverage; template stays thin. No component
  framework installed — deliberately not added for this size.
- Panel placement: new `config-block` on `ConfigPage` (`v-if` admin), not a
  new route. Self row: enabled with strong confirm when other admins exist,
  disabled with a "promote someone first" hint when sole admin. Demote
  requires a confirm dialog; toggle reverts on failure (401 → login
  redirect, else server message verbatim). Stale-role display until remount
  is accepted (server enforces regardless).
- Login: add a User ID field (required), post `{ userId, password }`; works
  in open mode too. Remember last successful id in `localStorage` for
  prefill.
- One Playwright scenario (admin login → toggle → assert) pins the wiring;
  included if cheap, else explicitly deferred with reason in the report.

### Steps

- B1 — Backend endpoint TDD: one RED→GREEN test per rule (unknown 404,
  zero-admin outcomes 409 both self and other, no-op 200, non-admin 401,
  share-credential 401, unauthenticated 401, bad id 400, read-only 405).
  New atomic `set_admin_role` helper (check+write in one redb write txn) +
  audit log line; hashing untouched (no password involved).
- B2 — Frontend: `api/users.ts`, lazy-computed role + pure predicates with
  vitest, `UserManagement.vue` + `ConfigPage` block gated on admin, login
  user-id field + last-id prefill, Playwright wiring scenario (or explicit
  deferral).
- B3 — `openapi.json` regen + `openapi-check`; full backend suite;
  `frontend-check` (prettier, vue-tsc, eslint) + `frontend-vitest`;
  `docs/auth.md` guard-table touch; plan `done`.

## Progress

- B1 done: `PUT /put/users/admin` + atomic `set_admin_role` + audit log,
  committed.
- B2 done: frontend panel + login, committed. Review caught one
  upgrade-blocking gap (legacy deployments unreachable from UI) fixed via
  login fallback (object → string → object).
- B3 pending: openapi regen/check, full suite, this reconcile, plan `done`.
