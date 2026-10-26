---
status: done
type: feature
priority: high
area: frontend
---

## Notes

Hand-written Playwright files (plain `.ts`, not specs: `first-admin-registration.ts`,
`login-throttle.ts`, `invite-registration.ts`) bypass the YAML
scenario pipeline, contradicting `docs/playwright_generator.md` ("Rather
than writing Playwright tests by hand ... this project drives them from
YAML scenario files") and the single-pipeline description in
`docs/test-strategy.md`. The general design (DSL verbs) must come first;
individual cases are then declarations in it, not bespoke code that gets
adjusted until green.

## Naming rule

`*.spec.ts` is the executable spec machinery: the scenario YAML files
plus `interpreter.spec.ts`, which runs them. Hand-written browser tests
are tests, not specs, and must be named `*.test.ts` so the suffix never
implies equal standing with the DSL pipeline. The long-term direction is
zero hand-written browser files.

## Implementation steps

1. Re-read `docs/test-strategy.md`, `docs/playwright_generator.md`,
   `docs/scenario-dsl.md` and derive the verb gaps from them, not from
   existing hand-written files: invite creation/redeem (`invite:` given
   with bound/open variants, exposing the token as a `$var`), second
   (non-admin) user seeding plus viewer-session login, API asserts under
   a chosen session (anonymous vs viewer vs admin), and whatever
   `login-throttle` needs (custom backend env, response observation).
2. Extend the DSL: `types.ts` Zod schemas, `schema.json` copy,
   `executeGiven` (invite minting, viewer seeding), interpreter verbs,
   `docs/scenario-dsl.md` reference. One verb at a time, each with a
   YAML scenario that fails before the verb exists.
3. Port the three hand-written flows to YAML scenarios; delete the
   hand-written files once their scenarios pass. Rename any remaining
   hand-written browser file to `*.test.ts`.
4. Update `docs/test-strategy.md` only after the port: single-track
   statement holds again, no second track to document.

## Settled decisions (2026-10-25)

- `server:` keys use `AppConfig` field names (`backend/src/model/config.rs`
  `config_tier_table`): `bootstrap_admin` (= `PICASU_BOOTSTRAP_ADMIN`),
  `login_ip_base_ban_secs` (= `PICASU_LOGIN_IP_BASE_BAN_SECS`), plus
  env-only `pbkdf_iterations` (= `PICASU_PBKDF_ITERATIONS`). No invented names.
- Closed-install boot check is a DSL scenario (`server.bootstrap_admin: false`,
  `navigate: /` → `ui.route: /login`, `navigate: /register` → `ui.route: /login`).
  Requires `api.response` extended with `method` + `body` for the anonymous
  `POST /post/users/bootstrap → 401` probe (current verb is GET-only).
- Slices (one worker each, TDD red-first): S1 `server:` + `login:` + `index:` +
  `api.response` method/body + first-admin YAML (done, 4d2157b8); S2 `invite:`
  given + invite-registration YAML (done, 78100c3a/2bc57c59); S3 throttle
  response-observation (`api.statuses`) + `ui.disabled`/`ui.enabled`/
  `ui.text_hidden` + login-throttle YAML (done, 20e4ed46); S4 hand-written
  retirement + docs (done, 2e783ece). All four complete.

## Finding F1 (2026-10-25, RESOLVED 2026-10-26 by viewer-tier-access S1):

viewers could not load any grid — `POST /get/prefetch` 401'd via the
admin-only cookie fallback and the interceptor bounced them to /login.
Verified fixed end to end: `non-admin-cmd-user.yaml` and the full
`invite-registration.yaml` (logout → login-as-maria → view) pass, full
suite 47 green. Details below are historical.

- DSL proof: `invite-registration.yaml` registers via invite, lands on
  `/timeline`, reload keeps the session — then a fresh viewer left idle on
  `/timeline` navigates itself to `/login` within 30 s (idle-only temp
  scenario reproduced it with zero test activity; admin idling 30 s stays
  put). CI-green hand-written `invite-registration.ts` never observes this:
  its assertions all complete within ~1 s of mount, before the bounce.
- Mechanism (all verified): timeline mount fires `POST /get/prefetch` as
  the viewer → `GuardShare` falls back to `try_jwt_cookie_auth`, which is
  admin-only ("User is not an admin" in the backend log) → 401 → the axios
  interceptor (`axiosInterceptor.ts` 401 branch) clears the profile and
  pushes `/login`. Same class as the fixed config bounce (95655073), but
  `fetchTags`/`fetchAlbums` (`GuardAuth`) and prefetch all still bounce.
- Pinned intent: `auth_users.rs::guest_share_prefetch_get_data_and_serving`
  asserts a non-admin without share headers is 401 on the share-guarded
  route ("otherwise the share guard would be vacuous"); `docs/authz-model.md`
  §4 denotes `GuardShare` as admin-cookie-or-live-share only. There is no
  viewer read capability in the model.
- Frontend-only fixes cannot complete the story: skipping tags/albums for
  viewers (done in `usePrefetch.ts` behind `shouldLoadAdminLists`, unit
  tested) removes two 401s, but the grid itself needs prefetch/get-rows/
  get-data, which 401 for viewers. The story "invite → register → login →
  view images" needs viewer reads at the guard level.
- Options (owner decision needed — backend auth behavior + formal model):
  (a) extend the data read path (prefetch/get-rows/get-data, plus serving)
  to accept a live non-admin `User` identity alongside share/admin, with
  `authz-model` + matrices updated and the "vacuous guard" companion
  reworked to viewer-vs-share terms; (b) keep backend tiers and declare
  viewers browse-less (contradicts the invite feature's purpose — not
  recommended); (c) stop-gap: nothing ships until (a) is decided.

## Decisions (2026-10-26, owner — F1 direction settled, see `.plan/viewer-tier-access.md` for the build)

- Viewer reads: option (a) — extend the `GuardShare`/`GuardUpload` cookie
  fallback to any live `User`; viewer snapshots policy-free, trashed
  exclusion stays in the query filter.
- Content writes are user acts, not admin: any authenticated user may
  upload, move, trash, delete, and describe photos and albums
  (`upload` cookie leg, `assign_album`, `delete-data`, `edit_flags`,
  `create_dir_album`, `set_album_cover`, `rotate-image`,
  `regenerate-thumbnail`, `edit_tag`/`edit_rating`). Design doc already
  grants moves/names/images/confirmation-gated deletes; guards catch up.
- Share passwords stop leaving the backend: drop `password` from reads
  (presence via `hasPassword`), `edit_share` absent/null means keep,
  string sets. Replaces the earlier redaction-blocker idea.
- Config cleanup (committed 9ac1e1c7): `imagePath` gone from
  `GET /get/config` (host-only, admin UI reads it from
  `GET /get/admin/config` entries); `uploadFolder` unredacted (relative);
  `hasAuthKey` gone (no consumer; reset POST still reports); `disableImg`
  removed backend-wide, now a frontend-only pref (constStore +
  FrontendConfig toggle). `GET /get/config` keeps zero admin-only fields,
  so its gate demotion is payload-free.

## Confirmation (2026-10-25): not invite-specific

- New `scenarios/non-admin-cmd-user.yaml` (+ `cli_user:` given verb for
  offline `picasu-cmd` provisioning in phase A, before backend launch;
  requires `server:` with `bootstrap_admin: false` — fail-fast otherwise).
- Result: the CLI-seeded admin logs in and sees `photo.jpg` on the
  timeline (control green — fixture, indexing, and CLI provisioning all
  work); the CLI-seeded viewer logs in, reaches the `/timeline` shell, and
  the grid stays empty — `ui.chip_visible: photo.jpg` finds nothing in
  15 s. Same failure with zero invite involvement, so the gap is "all
  non-admin users are untested and broken", not "invites are broken".
- The file is committed failing (no DSL skip mechanism exists) as the F1
  regression pin: full suite reads 43 pass + 1 fail until the tier
  decision lands. Temp idle-probe scenarios were removed after confirming.
- 2026-10-26: S1 landed (cookie fallback accepts any live user).
  `non-admin-cmd-user.yaml` renamed to "... uses the gallery" and goes
  green end to end (admin grid → viewer login → viewer grid tiles →
  image view); invite story legs re-attached (logout → login-as-maria →
  view). Full suite 47 passed, 0 failed.

## Progress (2026-10-25, S1 done)

- `server:` (`bootstrap_admin`, `pbkdf_iterations`,
  `login_ip_base_ban_secs`; absent keeps harness default), `login:`,
  `index:` verbs + `api.response` `method`/`body` in `types.ts`,
  `schema.json`, `interpreter.ts`, `interpreter.spec.ts` (scenario-owned
  backend via `serverExtraEnv`); `docs/scenario-dsl.md` reference.
- New: `scenarios/first-admin-registration.yaml`,
  `scenarios/closed-install-boot.yaml`. RED watched (unrecognized
  `server`/`login`/`index` keys) before GREEN.
- E2E finds: (1) `fill: textbox/Password` strict-mode violation on
  RegisterPage (`Password` vs `Confirm Password`) — resolver now prefers
  the exact-name hit when it resolves to one element; (2) blanket
  `exact: true` regressed `click: button/3` (rating stars match by
  substring) — exact-prefer-with-substring-fallback keeps both green.
- Gates: 2 new scenarios pass; full `npx playwright test` 45 passed;
  eslint, `vue-tsc -p tsconfig.node.json`, prettier, `plan-lint` clean.
  Backend untouched. Leftover: given-phase auto-index heuristic still
  present; fixture backend still boots under `server:` scenarios (wasted,
  torn down by fixture); `invite:`/throttle verbs open (S2/S3).

## Progress (2026-10-25, S2 DSL done, story blocked by F1)

- `invite:` given (bound `username` vs open, `expires_in_hours` default 168,
  token exposed as `${id_as}`) in `types.ts`, `schema.json`,
  `executeGiven.ts` (mints via admin API after the `user:` given);
  `reload: true` verb; `api.response` body deep-interpolation;
  `click.menu: <text>` verb for overlay menus (list-item roles are
  unreliable there — hand-written specs already use `getByText`);
  `resolveLocator` prefers the exact accessible name (waits briefly for
  attach first: Vuetify binds names async), substring fallback kept for
  `button/3`-style matches.
- `scenarios/invite-registration.yaml` pins prefill, anonymous mismatch
  403, UI redeem → `/timeline`, reload, reuse 409, unknown 404, open-invite
  register, viewer tier (`GET /get/users → 401`). Logout/login/view-image
  legs are staged in the file footer as comments until F1 is decided.
- E2E finds: (3) `api.response` posted the raw body uninterpolated, so an
  invite redeem sent the literal `${bound}` text and answered 404 instead
  of the binding 403 — bodies now go through `interpolateDeep`; (4, latent)
  `assertionTarget` does not interpolate `ui.route`, so `covers.ui` cannot
  name parameterized routes — worked around with static targets (advisory
  warnings are not gates); (5) `click: option/<item>` never resolves in
  teleported `v-menu` content — `click.menu` matches exact visible text in
  `.v-overlay` instead; (6) exact-name `count()` raced Vuetify's async
  label binding — resolver now waits for attach before falling back.
- Frontend viewer-skip for the admin-only tags/albums lists
  (`shouldLoadAdminLists` in `currentUser.ts`, unit-tested, wired into
  `usePrefetch.ts`): removes two certain 401s, but does NOT unblock the
  story — the grid itself needs prefetch, see F1.
- Gates: scenario passes with zero coverage warnings; eslint,
  `vue-tsc -p tsconfig.node.json`, prettier, `plan-lint` clean. Backend
  untouched. Leftover carried: auto-index heuristic, fixture-backend waste,
  throttle verbs (S3), hand-written deletion + doc updates (S4).

## Out of scope

Backend behavior changes. New product features. The viewer denial-matrix
coverage from `e2e-viewer-invite-coverage.md`, which becomes YAML
scenarios under this plan once the needed verbs exist.

## Completion (2026-10-26)

All four slices landed; hand-written browser files are gone (the last
three retired in 2e783ece) and coverage declarations across the corpus
were corrected to real assertion targets (b54bac1c). Full suite 46
scenarios, zero coverage warnings.
