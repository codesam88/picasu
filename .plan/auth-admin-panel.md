---
status: done
type: feature
priority: high
area: frontend
---

# Auth admin panel: users, passwords, invites

Extracted from `.plan/auth-flows-enrollment.md` step 6 (admin panel UI).
Self-change in settings (step 5), the enroll flow, and endpoint semantics
(bootstrap/register/invite backend) are already done there; this plan owns
the admin-facing UI and its small backend read-back additions.

## Problem

Admins have no UI for user/invite management. `PUT /put/users/admin`,
`PUT /put/users/password`, and the invite routes exist but have no SPA
callers; `createUser` in `api/users.ts` is caller-less (R22 in
`.plan/auth-enrollment-review.md`). Admin UX otherwise requires the CLI or
manual API calls.

## Scope

- Admin-create-user form: username (+ initial password, admin-forced
  non-admin) targeting `POST /post/users/register` with an admin session
  (invite not needed then; never grants admin). Wire or delete the
  caller-less `createUser` — no dead export remains.
- Set-password control per user (admin → other keeps no-old recovery;
  self-change stays in `ChangePassword.vue`).
- Elevation control (`PUT /put/users/admin`) if not already reachable in
  the UI; deny_unknown_fields is pinned backend-side.
- Invite management: create (return link/token once), list with status
  (unused/used/expired/revoked), revoke.
- Registration toggle: **decision needed** — the original step-6
  requirement predates the R12 host-only extension, which now rejects any
  `allowRegistration` presence in `PUT /put/config` with 400. Options:
  1. Read-only display of the host-managed value + pointer to the
     setter (`PICASU_ALLOW_REGISTRATION` / config file); admin UI never
     writes it.
  2. Reopen the host-only decision for this one knob (contradicts the
     settled R12 decision; needs justification).
     Default is option 1 unless the owner reopens the decision.

## Backend additions (small, in this change)

- `ConfigResponse` (`GET /get/config`): read-back for the registration
  state the panel needs (and any invite-list prerequisites). Note this
  endpoint is also R13 (absolute-path disclosure via `GuardShare`) — do
  not add fields that widen disclosure; narrowing R13 may land alongside.
- Openapi regen + oasdiff pins if response shape changes.
- No changes to bootstrap/register/invite write semantics.

## Test cases

Frontend (vitest + component):

1. Admin-create form: success ⇒ user listed; duplicate ⇒ 409 surfaced;
   non-admin session ⇒ route/control hidden.
2. Elevation toggles admin flag; failure message shown.
3. Set-password: success clears control; 4xx surfaced, no login redirect.
4. Invite create ⇒ link shown once (refresh loses it); list renders
   states; revoke ⇒ state flips.
5. Registration display reflects read-back; no PUT attempted for
   host-only knobs.

Backend: extend config/knob matrices only for new read-back fields.

## Steps (one worker item each, TDD red-first, no commits by worker)

1. Decision on the registration toggle (option 1/2 above), then
   `ConfigResponse` read-back + openapi regen + tests.
2. Admin panel UI: users list/form (create, elevate, set-password) via
   existing `api/users.ts`; resolves R22.
3. Invite management UI against `invites` routes (create/list/revoke).
4. Docs: admin-panel section in the user-facing docs and `docs/auth.md`
   where flows change. Gates: `docs-check`, `plan-lint`.

## Gates

vitest, `frontend-check`, `openapi-check` after regen, `cargo test` +
`backend-check` for backend changes, `docs-check`.

## Outcome (2026-10-27)

All four steps done. The registration-toggle decision resolved as
**option A (writable)**: the ticket's option-1 rationale was conditioned
on R12 rejecting `allowRegistration` in `PUT /put/config`, but that route
is gone — `PUT /put/admin/config` accepts the knob (pinned by backend
test, `docs/config.md` tier table lists it runtime-writable), so the
premise never carried to config-v2 and the panel keeps the working
toggle. Step 1's read-back already existed via the admin endpoint.

`createUser` (R22, caller-less) is now wired by the admin create form
(register with an admin session never grants admin — verified against the
handler); set-password prompts for the caller's own password for the
`X-Picasu-Password` re-auth header (403 surfaced, no redirect); invites
gained per-row status derivation (extracted to `inviteStatus.ts`, six
unit cases) and an optimistic revoke flip; elevation was already
bidirectional. Pins live at the api/util layers; `.vue` glue follows the
house structural-pin convention (node vitest, no mount harness — the
reasoning is recorded in the new test file's header, matching
`userManagement.test.ts`).
