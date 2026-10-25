---
status: in-progress
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
  `api.response` method/body + first-admin YAML; S2 `invite:` given + viewer
  session + invite-registration YAML (expect backend/frontend bugs to surface);
  S3 throttle knob + response observation + disabled assert + login-throttle YAML;
  S4 docs + hand-written cleanup + full gates.

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

## Out of scope

Backend behavior changes. New product features. The viewer denial-matrix
coverage from `e2e-viewer-invite-coverage.md`, which becomes YAML
scenarios under this plan once the needed verbs exist.
