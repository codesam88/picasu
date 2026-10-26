---
status: done
type: feature
priority: high
area: frontend
---

## Notes

E2E coverage for the invite-registration → viewer-session flow and the
admin-surface denial matrix for non-admin users. Two production bugs
shipped without any e2e catching them (wrong prefill on `/register?invite=`,
viewer bounced to login by the admin-only `GET /get/config`); the
playwright suite has zero invite and zero viewer coverage.

## Why nothing caught it

- `given: user:` only bootstraps the first admin; there is no invite
  verb and no way to seed a second (non-admin) user.
- Harness-managed sessions always log in as the admin account
  (`resolveSessionCreds`); no scenario ever holds a viewer session.
- `GET /get/config` viewer-401 was pinned as intended backend behavior
  (`api_config.rs` R13 test), so the frontend loop had no failing gate.
  -structural unit tests pin source strings, not the running flow.

## Scope

1. [superseded 2026-10-26] The hand-written spec was replaced by
   `scenarios/invite-registration.yaml` (broader: adds logout/login/view
   legs) and the file deleted. Original description follows for history:
   Spec-style invite flow test (`invite-registration.spec.ts`,
   passing): admin mints a username-bound invite via API, viewer
   registers through `/register?invite=&username=`, prefill shows the
   bound name (not the admin's last-login id), register lands on
   `/timeline`, reload stays, login works, seeded photo is visible.
   Corner cases: dropped `username` param still registers, wrong name
   → 403 + unconsumed invite, reused token → 409, unknown token → 404,
   open (unbound) invite registers any id. Note for future specs:
   Playwright's `request` fixture keeps a cookie jar, so anonymous
   redeem attempts need `playwright.request.newContext()`.
2. [superseded by 2b below — never built in spec form; the route list
   went stale when content writes demoted to user-level] Spec-style
   viewer denial matrix (`viewer-access.spec.ts`): for every
   admin-level route/guard (`GET /get/users`, `GET /get/invites`,
   `POST /post/invites`, `DELETE /delete/invites/*`, `PUT /put/users/admin`,
   `PUT /put/users/password` cross-user, `PUT /put/admin/config`,
   `GET /get/config/export`, `POST /post/index/*`, `DELETE /delete/*`),
   assert a viewer cookie is refused and the UI keeps the viewer on
   `/timeline` (no logout bounce). Assert admin-only UI (Administration
   tab, user management, invite panel, scan buttons) is hidden/disabled
   for the viewer.
3. [done] YAML harness support: `invite:` given verb ships (bound/open,
   token as `${id_as}`); the whole flow now lives in
   `scenarios/invite-registration.yaml`.
   2b. [done, rescope 2026-10-26] Built as `scenarios/viewer-admin-denial.yaml`
   after the viewer-tier change: the original admin-route list went stale
   (content writes demoted to user-level), so it now probes only the
   surfaces that stayed admin and pins the Administration/scan UI as
   hidden — which also exposed and fixed the StorageAndSync config-page
   session bounce for viewers.

## Out of scope

- Backend behavior changes (viewer tier on `/get/config` stays as is).
- Share-link flows (already covered).
- Backend `tests/register.rs` already covers invite API corners; e2e
  asserts the wired UI outcome, not each API status again.
