---
status: in-progress
type: feature
priority: high
area: backend
---

## Notes

Working spec for the settings/config API v2. Old config routes
(`GET /get/config`, `GET /get/config/export`, `PUT /put/config`,
`POST /post/config/import`) are deprecated with no compatibility
obligation: remove, don't alias. Builds on the tier assignment in
`.plan/settings-consistency.md`; fixes the holes behind
`.plan/bug-readonly-lockout.md` and `.plan/auth-hardening.md` F9 by
construction rather than patch.

## Tiers (settled 2026-10-24, corrected 2026-10-25)

1. `host`: file + env (+ CLI bridge), restart to apply. Never writable
   via API. Readable via API only as annotated metadata
   (`writable: false`). Includes: `address/port`, `data_home/image_home/
web_root`, `trustProxyForwardedFor`, all 12 `login_*` throttle knobs,
   `trivialPasswords`, `PICASU_PBKDF_ITERATIONS`, `bootstrapAdmin`.
   Throttle knobs are not API-configurable.
2. `runtime`: admin API, persisted to `config.toml`, effective without
   restart. Includes: `uploadFolder/maxUploadSize`, `readOnlyMode`,
   `disableImg`, `fsNotifyWatcher`, `normalizeUploadFilenames`,
   `validateUploadContent` (plain `L2`, no extra confirm),
   `useClientTimestampInfo`, and `allowRegistration`.
3. `user`: per-user prefs writable by self. Current set is empty: `theme`,
   density/layout stay frontend-only (`localStorage`/IndexedDB) and get
   no route. A future roaming pref lands here.
4. `client`: frontend-only, never a server knob.
5. `secrets`: file + env only, never API (except dedicated rotate).
6. `identity`: `userId/admin/home`, password hash. Readable in
   profile (`userId` naming, no separate `username` field); writable
   only via dedicated identity routes / future admin user-management
   panel, never via settings.

Every `AppConfig` + user-setting field names its tier in one source
array; unlisted settings fail the table test. Docs table is generated
from the same array. `PICASU_PBKDF_ITERATIONS` is env-only (not an
`AppConfig` field): listed as out-of-schema host, not covered by the
table test.

### Env-overlap rule (decided 2026-10-25)

Env overrides are read at startup and take precedence over the file.
Where a runtime setting overlaps an env var (`readOnlyMode`,
`allowRegistration`, `disableImg`, `fsNotifyWatcher`, `uploadFolder`,
`maxUploadSize`), the API write persists to `config.toml` and takes
effect in-session, but is reverted at the next restart while the env
is pinned. This is intended host-override behavior. `GET
/get/admin/config` reports `source: file | env` per field so the UI
can warn instead of pretending persistence.

## Access levels

- `L1` user token, self scope, no re-auth, allowed while read-only is on.
- `L2` admin token, no extra step.
- `L3` admin token + explicit UI confirm (destructive/reversible in-session).
  `readOnlyMode=false` is `L3`, decided 2026-10-24, corrected 2026-10-25:
  the confirm calls a dedicated disable route that bypasses
  `GuardReadOnlyMode` (single named exception, `GuardAuth(admin)` only).
  No password header anywhere.
- `L5` operator only, no API write.

No `L4` (password re-auth) is used anywhere in this plan. Password
change keeps its existing `old/new/confirm` dialog payload, not a
re-auth header.

## Routes (polished)

Prefix convention (`/get|/put|/post/...`) is kept; the second segment
names the actor/scope, not the HTTP verb:

- `GET /get/admin/config` — full view for admin UI. Every field carries
  `{ value, tier, source, requiresRestart, writable }`. Host fields are
  `writable: false`. Replaces the need for the UI to guess `400` hints.
  `GuardAuth(admin)`. No `GuardReadOnlyMode` (reading is always allowed).
- `PUT /put/admin/config` — patch runtime tier only, including
  `allowRegistration`. Absent = keep. Unknown or host/managed field =
  `400` with setter hint. Validates paths and runtime ranges (not
  throttle ranges — throttle knobs are presence-rejected). Returns
  applied document + `restartRequired: []`. `GuardAuth(admin)` +
  `GuardReadOnlyMode`. Replaces `PUT /put/config`. There is exactly one
  named exception to the read-only rule (see next bullet).
- `POST /post/admin/config/disable-read-only` — the exception. No body
  (or `{}`); sets `readOnlyMode=false` only. `GuardAuth(admin)` only,
  no `GuardReadOnlyMode`. `L3`: the admin panel confirms before
  calling. This is what the `Server`-tab toggle calls when the mode is
  on; the ordinary `PUT` stays `405` while on.
- `GET /get/admin/config/export` — versioned envelope
  `{ version: 1, scope: "server-runtime", config }`. `GuardAuth(admin)`
  only: export works regardless of read-only mode. (`authKey` never in
  the document; `webRoot` hand-built, never serde-derived.) No import
  endpoint: removed from this plan as too complex (strict-schema /
  override semantics undefined). Revisit separately if backup-restore
  is needed.
- `GET /get/self/profile` — single user resource, two sub-objects:
  `{ user: { userId, admin, home }, settings: {} }`. Identity fields
  are read-only for the user (`userId` naming matches the wire
  contract); `settings` is empty until a roaming pref needs it. One
  round trip for boot; no separate `get/user/config`.
  `GuardUser` (any logged-in user). Allowed while read-only is on.
- `PUT /put/self/profile/settings` — reserved for future roaming prefs.
  `GuardUser`, no re-auth, no read-only guard. Not built until a pref
  needs roaming; `theme`/density/layout stay client-only.
- Identity mutations stay separate, never inside settings: password via
  the existing `old/new/confirm` dialog payload on
  `PUT /put/users/password`; `username/isAdmin/home` read-only until the
  admin user-management panel owns them (`admin-user-panel`). Role change
  keeps no-self-demotion; no `L4` header anywhere.

### Profile vs settings: decision

No separate user-config resource. Profile is the resource, settings is
the sub-object. Rationale: one boot read, one cache key, no
`profile vs config` naming drift; write paths stay split because
identity writes need re-auth and admin checks while pref writes do not.
If a pref ever needs per-field ACL, split the sub-object then, not now.

### Admin view of other users + invites (decided 2026-10-25)

Existing `GET /get/users`, `PUT /put/users/*` migrate under the same
convention when touched (`GET /get/admin/users`,
`PUT /put/admin/users/...`); not in scope to rename in this plan unless
the admin panel work wants it. No behavior change here.

Invites hook up end to end in a later phase (recipient page is missing:
nothing consumes `?invite=` today, `RegisterPage` is bootstrap-only):

- `RegisterPage` accepts an `invite` token and posts it to
  `POST /post/users/register` alongside `{ userId, password }`.
- Minting stays `GuardAuth(admin)` (`POST /post/invites`, still
  `GuardReadOnlyMode`-gated as a mutation).
- Invite gains an optional `username` constraint set in the admin UI
  alongside `expiresInHours`: when non-empty, redemption is limited to
  that `userId`; an existing `userId` is refused as usual. Requires an
  `InviteRecord` field addition (no such constraint exists today).
- Token stays bearer-shown-once (hash stored, `prefix…` in listings).

## Frontend representation (first sketch, built 2026-10-24)

`ConfigPage.vue` grows a `v-tabs` bar: `Server` (everyone who can open
config: `StorageAndSync` + `AdvancedConfig`) and `Administration`
(`v-if="isAdmin"`). The admin tab stacks the existing `UserManagement`
(role toggles, self-demotion confirm) and the new `InviteManagement`
(`frontend/src/components/Page/Config/InviteManagement.vue` backed by
`frontend/src/api/invites.ts`): create with `expiresInHours` (default
168, `0` = never), one-time link `${origin}/register?invite=${token}`
with copy button, list (`prefix…`, `createdBy`, expiry/used state),
revoke. Auth-key `Reset Key` stays in `AdvancedConfig` for now; moving
it into the admin tab is follow-up once `GET /get/admin/config`
annotates it.

`allowRegistration` has no toggle yet: the backend still rejects it as
host-only (`400`). Wiring the toggle is blocked on the tier flip to
`runtime` in this plan.

## Critical security decisions surfaced

1. Invite tokens are bearer credentials shown once: only the blake3
   hash is stored, listing carries `prefix…` only, revoke takes the
   hash id. The copy-link UI warns accordingly; no resend/reveal.
2. `Reset Key` signs everyone out (existing copy kept); invites and user
   list refresh, no silent session reuse.
3. Role demotion keeps the confirm dialog, including the self-demotion
   warning; backend refusal of zero-admin / self-demotion is
   authoritative, the UI lock is advisory.
4. `readOnlyMode` toggle stays on the `Server` tab but submits through
   the ordinary `PUT`; lift while on is `405` until the `L3` confirm
   flow lands with the new `PUT /put/admin/config`. No password header
   is collected anywhere in this sketch.

## Leaves out

- New gallery settings themselves (land in `runtime` by default).
- Frontend-only keys (never get a route).
- Migration/shim for old routes (deleted outright per deprecation),
  except phased `GET /get/config`: the client-safe view stays until
  the frontend moves share/non-admin boot (`readOnlyMode`, `disableImg`,
  `imagePath`, `hasAuthKey`) to `GET /get/self/profile` or a
  share-admitted successor; then delete.
- Config import: no endpoint in this plan (too complex right now).
- Invite recipient page + `InviteRecord.username` constraint: phased
  follow-up (see above).

## Documentation rule (decided 2026-10-25)

`docs/config.md` is normative for precedence and tiers — not the plan
files. On every change in this plan, update it in the same commit to the
new status (no drift, no "to be documented later"):

- Precedence + overlap: env > file for the documented vars; API writes
  persist to `config.toml` and apply in-session but revert next restart
  while the env is pinned (intended host override). `GET
/get/admin/config` `source` field mirrors this per-field.
- Tier table: each knob names `host` / `runtime` / `env-only`, how to set
  it (file key, env var, flag, API route), and whether a restart applies
  it. Host-only vs env-only is explicit (`PICASU_CONFIG_HOME`,
  `PICASU_PBKDF_ITERATIONS`, `PICASU_RATE_LIMITS` are env-only by
  construction).
- Perspective: concise, user-facing. Architecture + usage reference +
  security specs (`auth.md`, throttling, invite bearer rules, reset-key
  sign-out, read-only exception). No plan chatter, no progress logs, no
  pointers at `.plan/` files in user docs.

## Implementation steps (executable, in order)

Each step is one worker task: TDD (failing test first), one commit,
`docs/config.md` updated in the same commit, `just openapi-gen` +
`just openapi-check` green where routes change. Old routes stay mounted
until the frontend migrates; deletion is the last step.

- [x] S1 tier table: single source array in `backend/src/model/config.rs`
      (every `AppConfig` field → `host|runtime`; `PICASU_PBKDF_ITERATIONS`,
      `PICASU_CONFIG_HOME`, `PICASU_RATE_LIMITS` listed as out-of-schema
      env-only). Table test fails on unlisted fields. `docs/config.md`
      tier/precedence/overlap tables generated-or-mirrored from it.
      Checks: `cargo test config`, `just docs-check`.
- [ ] S2 `allowRegistration` → runtime: remove from `host_only_knobs` +
      add `Option<bool>` to `PartialUpdateConfigRequest` with apply logic;
      route test pins accept + persistence; old `400` tests updated.
      `docs/config.md` row flips host → runtime. Checks: `cargo test
put_config`, `just backend-check`.
- [x] S3 new admin routes alongside old: `GET /get/admin/config`
      (annotated `{value,tier,source,requiresRestart,writable}`,
      `GuardAuth(admin)`, no read-only guard), `PUT /put/admin/config`
      (runtime patch incl. `allowRegistration`, `GuardAuth` +
      `GuardReadOnlyMode`), `POST /post/admin/config/disable-read-only`
      (`GuardAuth` only, sets `readOnlyMode=false`), `GET
/get/admin/config/export` (versioned envelope, `GuardAuth` only).
      Handler tests per Acceptance 2–3 incl. guard absence/presence
      assertions. `just openapi-gen`, `just openapi-check`.
- [x] S4 `GET /get/self/profile` (`GuardUser`, read-only-safe):
      `{user:{userId,admin,home},settings:{}}` from user store + empty
      settings. Handler tests (self, non-admin, share-token 401, works
      while read-only on). Frontend not rewired yet.
- [ ] S5 frontend wiring: `ConfigPage` tabs call the new routes;
      `Server`-tab R/O toggle calls disable route with `L3` confirm when
      on; `allowRegistration` toggle in admin tab; export button.
      `frontend/src/api/*` zod-parsed. Checks: `just frontend-check`,
      `just frontend-vitest`.
- [ ] S6 cutover: frontend drops old paths, old routes
      (`PUT /put/config`, `POST /post/config/import`) unmounted, `GET
/get/config` kept (phased boot migration). `docs/config.md` +
      `auth.md` final pass, `just check`, `just test` (backend + frontend
      vitest; playwright only if touched).

## Acceptance

1. Table test: every `AppConfig` field names its tier; unlisted fails.
   `allowRegistration` is `runtime` (new `PUT` accepts it, host-only
   entry + `400` test removed, route test pins accept);
   `trivialPasswords`/`bootstrapAdmin`/throttle knobs are `host`
   (`PICASU_PBKDF_ITERATIONS` listed as out-of-schema env-only);
   `theme`/density/layout have no tier (client-only, no route).
2. `PUT /put/admin/config` with a host field = `400` + hint. `PUT`
   validates paths and runtime ranges (no throttle-range claim).
3. Read-only on: ordinary `PUT /put/admin/config {readOnlyMode:false}`
   = `405`; `POST /post/admin/config/disable-read-only` with admin
   token = `200` and mode off afterwards (`L3` confirm client-side).
   Export works regardless of mode; there is no import endpoint.
4. `GET /get/self/profile` exposes `{ userId, admin, home }` read-only;
   no settings write path touches identity or host/runtime fields.
   `GET /get/config` stays until the phased boot migration lands.
5. `just openapi-gen` diff reviewed, `just openapi-check` green; old
   routes (`PUT /put/config`, `POST /post/config/import`) absent from
   the document; the disable route carries `GuardAuth` and no
   `GuardReadOnlyMode`.
6. `docs/config.md` (+ `auth.md` where touched) updated in the same
   commits: tier/precedence tables match the new behavior
   (`allowRegistration` runtime, throttle/PBKDF/CONFIG_HOME env-only,
   overlap rule), user-facing and concise.
