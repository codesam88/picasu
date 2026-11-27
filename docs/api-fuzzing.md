# API Fuzzing

Strategy home for API fuzzing and authorization assurance. Operational
commands, environment variables, debugging, and hook details live in
`utils/fuzz/README.md`; this document covers the strategy, oracles, gates, and
known limits. The contract that feeds the harness is described in
`docs/api-assurance.md`.

## Purpose and scope

The goal is to exercise the same API as several principals so the fuzzer finds
broken access control, not just crashes. Coverage spans vertical escalation
(low role to high role), horizontal access (same role, different owner), and
tenant separation (same role, different scope), plus missing-auth and
read-only bypasses.

Two complementary mechanisms are used:

- A deterministic five-check oracle sweep (A1-A5) over every operation the
  contract assigns a check to, gated in CI.
- A generative stateful campaign that sequences operations and random inputs,
  run per role with a fixed seed and triaged against a baseline.

The harness is a consumer of the contract, not its source of truth. Auth
knowledge is derived from `backend/auth-matrix.json`; findings flow back as
contract corrections or triaged baseline entries.

## Threat model

Principals under test:

- **Admin** — full privileges, crosses owners and albums.
- **Member** — authenticated, non-admin, owns a subset of resources.
- **Delegated guest** — holds a share snapshot token scoped to one album.
- **Anonymous** — no credentials at all.

Attack goals the oracles target: reading or mutating another user's objects,
calling admin operations as a member or guest, escaping a share's album scope,
renewing a snapshot minted for a different share, bypassing read-only mode,
and reaching declared-auth operations with no or wrong credentials.

False-positive sources are handled as matrix rows rather than global rules:
admin legitimately crosses owners, OR-scheme operations admit several
credential kinds, and delegated paths need wrong-scope probes in addition to
no-credential probes.

## Oracles

### A1-A5 oracle sweep (Tier 1)

`just fuzz check` runs a bounded single-operation sweep against a throwaway
seeded backend and compares findings to `baseline.yaml`. Each operation the
contract assigns a check to carries exactly one of:

- **A1 `cross_scope_refused`** — a share-A snapshot token cannot read an
  album-B asset; the mismatched request answers `404` so the route does not
  reveal that the id exists.
- **A2 `demoted_token_rejected`** — a demoted user's still-live token loses
  admin access immediately; every admin-guarded session operation answers
  `401`.
- **A3 `wrong_credential_rejected`** — a session token does not satisfy a
  snapshot-token guard; every snapshot-typed bearer operation answers `401`.
- **A4 `readonly_405`** — every mutating (`GuardMutate`) operation answers
  `405` under read-only mode.
- **A5 `cross_share_renewal_refused`** — share B cannot renew a snapshot
  minted for share A; the album-scoped renewal operations answer `401`.

Probe targets are generated from the contract, not hand-picked. The sweep
gates CI.

### Differential oracle

The same operation is run as admin, member, guest, and anonymous with a fixed
seed, then outcomes are diffed by status, body shape, field inventory, and
side effects. Full bodies are not compared byte-for-byte: ordering,
timestamps, and session identifiers differ per request. Marker matching plus
order-independent record comparison is used instead.

### Missing-auth gate

A dedicated anonymous pass sends no credentials, plus an invalid-credential
variant, against every declared-auth operation. Refusal is required; a 2xx is
a finding. Multi-scheme (OR) operations are explicit matrix rows rather than a
global "non-admin must 4xx" rule.

### Body and side-effect markers

Oracles inspect body markers, not just status codes: some APIs return `200`
with a failure body. Denied mutations are also checked for side effects — a
denied request that still enqueues a job or writes a row is a failure even
with a `403`.

## Seed design

Accounts, shares, and a minimum object corpus are created out-of-band before
the run and never inside generative scenarios, which would race with guards.
Seeding fails closed: if any role has zero rows or objects, the run aborts.

Principles:

- One account per role, created through the real login flow, with credentials
  persisted to files the hooks read.
- One object per scope before the run, so object and asset endpoints have
  valid targets.
- Tokens are obtained via the declared login flow with extraction from cookie
  or body and `expectCookies` where applicable, not copied as static headers.
- Reseed by deleting the state directory; the next run recreates it.

Current target inventory (high level, exact counts may change with `seed.sh`):
admin0/admin1, memberA/memberB, sharing-disabled, and disabled accounts;
global alpha/beta/gamma, nested alpha/sub, and personal home albums; open,
nodownload, upload, password-protected, expired, member, dead, and nested
shares; valid, bound, and used invites.

The `seed.env` contract exposes the seeded state to other tools: `PICASU_JWT`,
`PICASU_BASE_URL`, `FUZZ_ALBUM_A`, `FUZZ_SHARE_A`, `FUZZ_ALBUM_B`,
`FUZZ_SHARE_B`, and `FUZZ_PASSWORD`. Fail-closed seeding asserts non-empty
snapshot rows before completing.

## Seed assumptions

Mechanics confirmed against backend (read-only, 2026-11-17):

- Expired invites: `POST /post/invites` accepts only `expiresInHours` `0`
  (never expires) or `1`–`8760`; negative and over-limit values are rejected.
  No API path yields a past deadline, so the expired-invite target is dropped
  (would require a direct redb insert).
- Personal homes: `PICASU_USERS_HOME` defaults to `<data>/users`;
  `ensure_personal_home` creates the directory plus its dir-album on user
  creation; indexing with `root: "personal"` (or the default `"auto"`) walks
  the users root and personal albums surface as `root: "personal"` filtered by
  owner.
- Nested albums: a global index creates dir-album records for every
  subdirectory, so `IMAGE_HOME/alpha/sub` gains its own `albumId` with
  `parentAlbumId` set to the parent album.
- Trash/rating: `PUT /put/edit_flags` body
  `{indexArray, timestamp, isTrashed?}`; `PUT /put/edit_rating` body
  `{indexArray, timestamp, rating}` (`0`–`5` or `null`). `timestamp` must be a
  snapshot id from `POST /get/prefetch` (`prefetch.timestamp`).
- PNG metadata: EXIF is read from the embedded eXIf chunk and XMP from
  embedded iTXt plus sidecar; PNG is not XMP-only.
- Share expiry: `exp: 0` never expires; `POST /post/create_share` stores `exp`
  verbatim with no bounds, so past values create already-expired shares.
- Disabled sharing: `PUT /put/users/sharing` toggles `shares_enabled`; shares
  whose creator is later disabled fail revalidation ("Share no longer
  exists"), i.e. dead shares. Seed by creating then disabling.
- Second admin: `POST /post/users/register` always creates a non-admin;
  elevate with `PUT /put/users/admin` `{userId, admin: true}` plus the
  `X-Picasu-Password` re-auth header.

## Distribution

One deterministic pass per role with a fixed seed, not one run with random
identity rotation. Admin, member, guest, snapshot, and anonymous passes get
equal run time and effort. Per-role runs are reproducible and attribute
findings to the identity that produced them; random rotation is neither
reproducible nor attributable.

WFC escalation (ordered try-first-user, then anonymous, then remaining users
until a 2xx/3xx, then locking that identity) is useful for mapping which
identity can call an operation, but it locks on first success and offers no
traffic-share knob, so equal per-role splits are built by separate per-role
runs. Peer-replay covers read-only cross-tenant BOLA on GETs with two real
accounts but is read-only by design and cannot replay writes.

## Gates

| Command / artifact            | Scope                                         | Gate status                             |
| ----------------------------- | --------------------------------------------- | --------------------------------------- |
| `just fuzz check`             | A1-A5 sweep vs `baseline.yaml`                | Gating in CI                            |
| `just fuzz matrix`            | One request per assigned check, deterministic | Gating; included in `just test`         |
| `just fuzz roles` / `diff`    | Equal-budget per-role passes, differential    | Advisory; `diff` fails on new diffs     |
| `just fuzz run`               | Stateful generative campaign                  | Advisory only                           |
| Nightly fuzz-advisory + SARIF | Campaign report converted to SARIF            | Advisory, uploaded under `schemathesis` |

`baseline.yaml` is a triage ledger, not a requirement set: it must shrink as
bugs are fixed, and a fixed bug makes its finding disappear. Promotion from
advisory to gating requires an empty differential diff on a known-good build,
stable per-role call counts, and a forced-invalidation staging run that
recovers via refresh or fails loudly.

## Contract derivation

Auth knowledge is derived, not hand-maintained:

`backend/auth-matrix.json` (committed machine contract of per-operation
`x-picasu-auth`) -> `utils/fuzz/auth_rules.py` -> campaign hooks.

The derivation covers share-header injection, check assignment,
wrong-credential token selection, read-only exemptions, and seeded scope
pairs. It is pure and unit-tested (`just utils-test`). A matrix change that
alters a probe fails the tests instead of drifting silently.

Destructive operations (logout, password and admin mutations, key rotation,
index rebuild, read-only toggle) stay disabled in `campaign.toml` and
`check.toml`, pinned to the matrix by a consistency test. Poison guards in the
hooks null config fields, protect the admin account, and redirect share and
delete operations to nonexistent fixtures so generative traffic cannot destroy
the session under test.

Regeneration is enforced: `just openapi-regen` regenerates the matrix (and the
rest of the OpenAPI-derived artifacts) and CI fails on a tracked diff, and a
missing or malformed matrix file fails the run.

## Refresh and reauthentication

The provider is not a static header. Short-lived tokens go stale mid-run, so
the supported pattern is a token-fetching provider with proactive refresh
(default interval 300 s) and reactive refresh on unauthenticated statuses
(`retry_on`, single refetch plus replay). Design hooks must surface "refresh
failed" loudly rather than continuing unauthenticated.

Constraints that shape the implementation:

- `retry_on` codes are pooled across all configured schemes, not per scheme.
- Only `call_and_validate`, the state machine, `run`, and `fuzz` replay; raw
  `call()` does not.
- Three consecutive fetch failures disable refresh for the run with a 30 s
  cooldown.
- If replay still returns a `retry_on` status, Schemathesis stops refreshing
  that operation; track and report rather than loop.
- Scope-aware caches key one entry per role or scheme derived from the
  operation's `security` stanza, so a member token cannot overwrite the admin
  entry.

Principal identity is tracked in scenario state, not in payloads: `setup` logs
in per scenario, `before_call` attaches headers from scenario state, and
`teardown` cleans up with the owner's credential. Payload-derived ids get
mutated by the fuzzer; scenario state does not.

The auth flow itself (login, logout, rotate key, delete user) is excluded from
the generative grammar and tested in a separate short auth-only campaign,
optionally unauthenticated, because fuzzing it with real credentials
invalidates the session under test.

## Limits and honest boundaries

- The built-in oracle ends at 5xx and schema conformance. Absence of effect,
  cross-operation isolation, token lifecycles, and races are custom checks or
  handwritten tests; the token choreography is not inferable from schema
  alone.
- Stateful campaigns are not byte-reproducible; they are time-budgeted and
  triaged, never gating.
- Admin false positives: a global "users must not touch each other's
  resources" rule fires on admin, moderator, and delegated-guest paths. Per-
  (role, object, action) expectations in the matrix are required.
- OR-scheme policies need per-endpoint rows; asserting "non-admin must 4xx"
  misfires where several identities legitimately succeed.
- Full-body comparison across roles is noise; compare shapes, markers, and
  side effects.
- Same-tenant accounts look like BOLA; horizontal tests need genuinely
  separate tenants or scopes.
- Expired credentials masquerade as authorization faults; verify credential
  validity before flagging 401s.
- Catch-all and soft-404 baselines: probe nonexistent paths first so guessed
  paths do not look like real endpoints.
- Body-level success on 200: oracles must inspect body markers.
- Shared caches keyed without the principal can return another user's object
  even when the controller check passes; include cache keys in the oracle
  where applicable.
- Token-independent operations defeat re-auth loops; track and report.
- Cookie-session logins defeat auto-detection entirely and need hand-written
  hooks.
- Stateful runs need budgets an order of magnitude past what reaches a handful
  of operations.
- Accepted findings need a baseline file before any gate can fail a build.

## See also

- `utils/fuzz/README.md` — operational commands, environment, and debugging.
- `docs/api-assurance.md` — contract basis and consumers.
- `docs/auth-matrix.md` — human-readable authorization matrix.
- `docs/test-strategy.md` — overall test layers.
