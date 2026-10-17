---
status: idea
type: feature
priority: medium
area: frontend
---

## Notes

Problem: user administration has API but no UI. `GET /get/users` lists,
`PUT /put/users/admin` toggles role, `PUT /put/users/password` covers
recovery, but admins cannot do any of this from the gallery; role changes
require direct API calls.

Solution strategy: admin-only user panel (list + role toggle) backed by the
existing routes, with one behavior change: `PUT /put/users/admin` must refuse
self-demotion outright (403/409 even when other admins exist), instead of the
current allow-when-covered transfer rule (`users_admin.rs` allows self-demotion
while another admin remains, refusing only zero-admin outcomes). Self-demotion
refusal prevents accidental lockout-by-click and keeps role changes explicit.
Rejected: client-side only guard (bypassable); keeping transfer-allowed
self-demotion (preserves the footgun this panel introduces at scale).

## Implementation steps

1. Backend self-demotion refusal (scope: `backend/src/router/put/users_admin.rs`,
   `backend/src/auth/users.rs` if needed): actor == target and `admin: false`
   is rejected regardless of cover; pin with handler tests (self-demotion
   with cover present, zero-admin case stays refused). Leaves out: recovery
   token, panel UI.
2. Panel UI (scope: `frontend/src/components/Page/Config/` or admin view,
   `frontend/src/api/users.ts`): admin-gated list via `listUsers`, per-row
   role toggle via `setUserAdmin`, error surfacing for refusal. Leaves out:
   password fields (recovery plan owns them).
3. Specs and docs (scope: Playwright `admin-user-management` scenario,
   `docs/auth.md`, openapi regen if status codes change). Leaves out: email
   or token flows.
