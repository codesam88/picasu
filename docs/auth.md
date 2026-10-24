# Authentication and Authorization

This document describes Picasu's authentication system: its goals, the paradigm
it follows, and the detailed mechanism as implemented in
`backend/src/router/auth.rs` and the route modules. The desired properties —
the model from which enforcement rules and tests are derived — live in
[`authz-model.md`](authz-model.md). Findings from the security assessment live
with the work that owns them (see Security assessment below); this document
keeps the mechanism only.

## Goals

Picasu is a self-hosted gallery with a deliberately small identity model. The
design goals, in priority order:

1. **Users, guests via shares.** Individual users sign in by id and password;
   admin is a role held by one or more users, not a separate account. Beyond
   users, guests exist only as **share links** — capability URLs that expose
   one album under an explicit policy.
2. **Shares are least-privilege by construction.** A share carries its policy
   with it — `showMetadata`, `showDownload`, `showUpload`, optional password,
   expiry — and every request through a share must be evaluable without server
   session state.
3. **No server-side session store.** The server must remain restartable and
   stateless with respect to authentication: nothing about "who is logged in"
   lives in memory or in the database. This keeps the backend self-contained and
   makes horizontal or crash-restart behavior trivial.
4. **Media access is capability-based, not just identity-based.** Knowing a URL
   must not be enough to read a file. Even a fully authenticated session must
   present a short-lived, per-asset token to fetch an image or an original.
5. **Self-hosted threat model.** The perimeter is the deployer's own network or
   reverse proxy. The system optimizes for "trusted family and friends" (see
   [Design Goals](design.md)), not for hostile multi-tenant internet exposure —
   this shapes several trade-offs called out in the assessment.

## Paradigm

The system is a **stateless capability-token architecture** composed of three
patterns:

- **JWT bearer/cookie authentication (HS256).** Identity and authorization
  statements are carried in signed tokens, never in server memory. One
  configuration secret (`auth_key`) signs every token type.
- **Tiered, short-lived capability tokens.** Tokens form a delegation chain: a
  long-lived _identity_ token (admin session or resolved share) mints a
  short-lived _snapshot_ token, which in turn mints per-asset _serving_ tokens.
  Each tier is scoped to one artifact (a query snapshot, a single file) and
  expires in minutes, so a leaked token has a small blast radius.
- **Request guards as the enforcement points.** Authorization is declared as
  Rocket request guards (`FromRequest` impls) on each route. Every guarded
  handler takes its guard wrapped in `GuardResult<T>` (`= Result<T, AppError>`),
  so a failed guard is caught and the handler propagates it with `?`; the guard
  list plus that propagation is the auditable authorization manifest of the
  API. A `GuardResult` binding whose rejection is not propagated leaves the
  route enforcing nothing, which is why the propagation is machine-checked (see
  `.plan/authz-check.md`). Guards that carry a resolved share (`GuardShare`,
  `GuardTimestamp`) are consumed as the witness: handlers derive the operation's
  target from the guard's claims rather than from the request body, so a share
  cannot act on another album (Findings F2–F4, now fixed).

Guests authenticate as **capability URLs**: the share's `albumId` + `shareId`
(traveling in headers or query parameters, plus an optional password header)
resolve server-side, per request, against the stored share record.

## Mechanism

### Secrets and token signing

| Secret       | Source                                                                                                            | Role                                                                              |
| ------------ | ----------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `auth_key`   | `<DATA_HOME>/auth/secrets.json` (owner-only file), or env `PICASU_AUTH_KEY` (in-memory override, never persisted) | HS256 key for **all** JWT types                                                   |
| fallback key | 32 random bytes, generated once per process                                                                       | used only when `auth_key` is unset; a restart invalidates every outstanding token |

The legacy `[secrets] password` and `[secrets] auth_key` config keys are
gone: an old `config.toml` still parses, but the values are ignored (a
legacy `password` triggers a boot warning). Passwords live per-user in the
password store; the signing key lives in the secrets file.

`AppConfig::get_jwt_secret_key` (`model/config.rs`) selects the key. All tokens
are HS256-encoded with it; validation pins the algorithm list to `[HS256]`
(`VALIDATION` in `router/auth.rs`, enforced by unit tests) so algorithm-confusion
attacks are excluded by construction.

### Sign-in

1. `POST /post/users/authenticate` takes `{ userId, password }`, trims the password,
   and verifies it against the user store (`<DATA_HOME>/auth/users.redb`) for
   that user id. A match mints a random `jti` (UUID v4) and sets a 14-day
   `Claims { role: User { id, admin }, exp, jti, ver }` JWT in an HttpOnly
   session cookie, returning the session profile `{ userId, admin }` in the
   body. **Nothing is written server-side**: the token is the session, and
   `ver` is the user's token version at mint time. Unknown users and wrong
   passwords both answer 401 with no distinguishing detail; the unknown-user
   path still pays one KDF round so timing leaks nothing.
2. The **server** issues the session cookie via `Set-Cookie`
   (`router/post/users/authenticate.rs`): `jwt=...; HttpOnly; SameSite=Strict;
Path=/; Max-Age=14d`, plus `Secure` iff the request arrived via
   `X-Forwarded-Proto: https` (a TLS-terminating proxy must forward that
   header; plain-HTTP LAN deploys get a non-Secure cookie). Page JS never
   sees the token, so a future XSS cannot exfiltrate the session (R5). The
   client keeps only the `{ userId, admin }` profile from the login body in
   its session store for display.
3. `GuardAuth` (and `GuardUser`) read the cookie, decode it with `VALIDATION`
   (expiry enforced) and check it against the state the server keeps:
   the identity record (existence, role, token version) and the revocation
   list. A token whose `ver` is not the user's current version was
   invalidated by a password change, a role change or an explicit
   invalidation; a token whose `jti` is in the revocation list was logged
   out. Both checks read the store on every request, so a revocation takes
   effect before token expiry. The admin role comes from the record, so a
   demotion takes effect at once. Tokens without a `jti` are rejected (hard
   cut — pre-`jti` tokens do not decode).
4. Logout is `POST /post/logout`, which decodes the presented `jwt` cookie
   with the same validation the guards use; on success it files that `jti` in
   the revocation list (until the token's expiry), then clears the cookie
   server-side (HttpOnly cookies cannot be removed by page JS) and answers 204. Missing, undecodable, or expired tokens skip the revocation but
   still clear the cookie and still answer 204 — idempotent with or without
   a live session. Only the presented device is logged out; other sessions
   survive. The client also clears its session profile and capability tokens
   unconditionally, so logout sticks even when the round trip fails.
   Password change (`PUT /put/users/password`, self-change and admin reset
   alike) writes the new hash **and bumps the user's token version in the
   same transaction**, so every outstanding token of that user dies with the
   old password and logs every device out. The same bump happens on a role
   change (`PUT /put/users/admin`): the role travels in the token, so the
   new role appears at the next sign-in.
5. Users are managed through `GET /get/users`,
   `POST /post/users/register`, `PUT /put/users/password`, and
   `PUT /put/users/admin` (admin-only). The first administrator is
   enrolled only by `POST /post/users/bootstrap` (`--bootstrap-admin` ∧
   empty store, forced admin); `register` always creates non-admin
   accounts — elevation is `PUT /put/users/admin` alone.

**Credential-guess throttling (brute-force resistance):** two endpoints
verify passwords, each with its own throttle instance so guesses on one
never move the other's budgets. Both check before running the password
KDF, so a throttled request stays cheap, and both count evaluated guesses
only — malformed bodies, permission refusals and unknown users record
nothing. The env-only switch `PICASU_RATE_LIMITS=false` bypasses both
(e.g. for schema fuzzing).

- _Login_ (`POST /post/users/authenticate`) keys three sliding-window
  buckets on `(client-ip, claimed-username)`: an IP-short burst bucket
  (3 failures per 10 min, first ban 10 s), an IP-long bucket
  per client address (16 failures per 24 h, first ban 30 min), and a
  user-long bucket per claimed username (18 failures per 24 h, first ban
  30 min, capped at 2 h). Past each threshold, bans land on every second
  over-threshold failure — the single failure between two bans is
  recorded but free, so two attempts after a served ban run with no ban
  time between them — and each ban doubles the previous one. The
  evaluated failure that opens a ban answers 429 itself (with
  `Retry-After` carrying the full rung delay); only non-tripping failures
  answer 401. A request whose ban is still active is likewise rejected
  with 429 and a `Retry-After` header (seconds; the max remaining ban
  across all three buckets).
  Buckets are keyed on claimed usernames whether or not the account
  exists, so throttling behaves identically for unknown names and reveals
  nothing about which users are real. A successful login drops the
  username entry and clears the IP burst bucket (the IP-long history is
  kept). All thresholds are knobs under `[server]` (see
  [Configuration](config.md) for the full table) and the client address
  honors `trust_proxy_forwarded_for`. The login page renders the 429 as a
  countdown banner and re-enables submission when the ban expires.
- _Password change_ (`PUT /put/users/password`, self-change branch only —
  admin resets verify no credential and are unthrottled) keys a dedicated
  instance on `(client-ip, session-jti)`: a lenient IP-short burst bucket
  plus an IP-long and a jti-long bucket of 3 wrong old-passwords per 24 h
  each. The first overstep revokes the attempting session (its `jti` joins
  the revocation list until the token's expiry) and answers 401, so the
  SPA logs out on that response; the cookie is dead on every route
  afterwards. Outside the kill-switch / clock-error bypass, each 400
  wrong-old-password carries the server-computed attempts left (`min`
  over the three buckets), so the UI counts down from that number instead
  of hardcoding the policy. A correct old password clears the session
  entry and the IP burst (the IP-long history is kept, as on login); the
  schedules are hardcoded beside the login constants, not operator knobs.

**What the two throttles buy:** the login buckets cap online guessing per
attacking host and per claimed account (including nonexistent ones, hence
no user-enumeration oracle) while the pre-KDF check keeps throttled
requests cheap and escalating bans slow persistent sprayers. The
password-change buckets bound old-password oracle queries per stolen
session (jti slot: a botnet rotating IPs against one session still shares
one budget) and per attacking host (ip slot: one host cycling stolen
sessions still shares one budget); revocation converts a spent budget
into session death, killing the stolen session itself. The instances are
separate so a login brute-force can neither lock the victim out of
changing their password nor be extended by password guesses — the
remediation action (change password, which bumps the token version and
kills all sessions) stays available mid-attack — while a legit user sees
the countdown and can stop before the session drops.

**Session-denial UX (frontend):** an unauthorized response outside the
share flow makes the axios interceptor clear the session profile and
navigate to the login page once; no snackbar is raised for it. The
login page carries a static, non-dismissing information banner (Vuetify
`v-alert`, the inline colored banner component — the auto-dismissing
queue widget is `v-snackbar-queue`) reading "Please log in to
continue". The wording is deliberately innocuous: the visitor may have
arrived without any session (deep link, bookmark), so the banner asserts
nothing about expiry. Repetition is safe by construction — the
navigation call is a no-op once the router is on the login route.

**Serving-token lifecycle (frontend):** the gallery's serving tokens
(snapshot and per-asset, 300 s TTL) are renewed on the main thread only —
`tokenStore` renews whenever a token is within a 120 s margin of its
expiry at hand-off time, so workers should never process a request with
a barely-valid token. A worker whose request is answered 401 reports
that once over its `tokenExpired` message channel (data worker: timestamp
token rejected; image worker: the rejected asset's id); the main-thread
handler force-renews server-authoritatively — the renewal endpoints
accept signature-valid tokens regardless of expiry — and recovery is
ambient (unfetched batches refill on the next scroll trigger, re-queued
images read the renewed token). No worker retries, and the worker realms
hold no escalation policy: if the session itself is dead, the force-renew
call 401s on the main axios instance and flows into the same
session-denial contract above. Share contexts are exempt from that
escalation by construction — the interceptor routes every status inside
the share flow to the share handlers (password modal / expired-link
state), never to the login redirect.

**Enrollment signal (frontend boot):** anonymous `GET /get/server-info`
answers 200 in every install state with exactly `{ enrolled,
enrollmentOpen, captcha: null }` (`Cache-Control: no-store`): `enrolled`
is true once any user exists, `enrollmentOpen` only on an empty store
with the bootstrap flag set, `captcha` always null (reserved). The
router guard reads the session profile as a fast path (present → allow,
zero fetches) and otherwise pings this endpoint fresh on each
sessionless navigation — needs-enrollment forces `/register`, anything
else falls through to `/login`; share routes are exempt. Nothing is
cached. Anonymous `GET /get/users` is 401 in all states, including the
empty store — it never serves enrollment state.

**Bootstrap mode (opt-in):** a fresh install is closed by default — with an
empty user store, `POST /post/users/authenticate` takes the unknown-user path
(401) and guarded routes reject callers: `GuardAuth` routes answer 401 and
`GuardUser` routes answer 403 (a privilege condition, not a session
failure — the SPA keeps its display state). No session exists
pre-enrollment. Online enrollment of the first administrator opens only
while the `--bootstrap-admin` flag (or `PICASU_BOOTSTRAP_ADMIN`, or the
`bootstrap_admin` config key) is set **and** the store is empty, and only
via `POST /post/users/bootstrap` (forced `admin: true`). The refusal
message names that endpoint while the window is open; with the flag off it
names the flag/CLI remedies instead. The flag is WARN-logged at
boot, the server refuses to start when the flag is set against an existing
`<DATA_HOME>/auth/users.redb` (bootstrap already happened, or the store
needs repair — provision via the CLI instead). This closes Finding F6 for the default
path; a network-exposed install is only ever open if the operator opted in.

**No legacy sign-in path:** the legacy single-password world is gone. A
bare JSON string body to `authenticate` answers 422 (well-formed JSON of
the wrong shape — Rocket's `Json` guard), never migration: no user is
created, nothing is enrolled. A legacy `password` key in an old
`config.toml` is ignored with a boot warning; empty installs enroll only
via the bootstrap route on an empty store or the `picasu-cmd` CLI.
Tokens predating the `jti` claim or the `Role::User { id, admin }` shape
do not decode.

### The three token types

| Type              | Claims                                                               | TTL   | Minted by                        | Consumed by                                                             |
| ----------------- | -------------------------------------------------------------------- | ----- | -------------------------------- | ----------------------------------------------------------------------- |
| `Claims`          | `role` (User{id, admin} / Share(resolved)), `exp`, `typ: admin`      | 14 d  | login, share resolution          | `GuardAuth`, `GuardShare` (cookie fallback), `GuardUpload`, `GuardUser` |
| `ClaimsTimestamp` | `resolvedShareOpt`, `timestamp`, `exp`, `typ: snapshot`              | 300 s | `POST /get/prefetch`             | `GuardTimestamp`, `TimestampGuardModified`                              |
| `ClaimsHash`      | `allowOriginal`, `hash`, `assetId`, `timestamp`, `exp`, `typ: asset` | 300 s | every row of `GET /get/get-data` | `GuardHash`, `GuardHashOriginal`                                        |

All three are plain JWTs signed with the same `auth_key`. Each carries a `typ`
claim and is decoded through a typed entry point that rejects a mismatched
`typ`; the claims structs also set `deny_unknown_fields`, so the three
cross-type decode paths fail closed (see Finding F1, now fixed).

What the server remembers about each of them — and why — is in
[Auth state model](#auth-state-model).

### Share resolution

`GuardShare` tries three sources in order:

1. **Headers** `x-album-id` + `x-share-id` (both or neither; half-supplied is a
   400), plus `x-share-password` when the share has one.
2. **Query parameters** `albumId` + `shareId`.
3. **Admin cookie fallback** — a logged-in user with the admin role passes any
   share-guarded route.

`resolve_share_internal` looks up the album in `METADATA_TABLE`, pulls the share
out of `shareList`, and calls `validate_share_access`: expiry (`exp > 0`) and
password equality. On success it builds `Claims::new_share(resolved_share)` —
the share record, including its policy flags, is **embedded in the token** for
the rest of the request chain. The share record is re-read from the database on
every request, so disabling a share takes effect immediately.

Policy flags are applied in two places: `resolve_show_download_and_metadata`
(`process/mod.rs`) turns `resolvedShareOpt` into `(showDownload, showMetadata)`
for data and metadata routes, and `prefetch` rewrites the query expression to
the share's album (`Expression::Album`) so a share can only ever snapshot its
own album.

### The media-serving chain

```
GuardShare (identity: who)
   └─ POST /get/prefetch ──────────────► ClaimsTimestamp (300 s, share policy embedded)
                                            │
                                            ├─ GET /get/get-data ── rows + ClaimsHash per asset
                                            │       (allowOriginal := showDownload)
                                            ├─ GET /get/metadata, get-rows, get-scroll-bar
                                            │
                                            └─ renew-timestamp-token ─► fresh ClaimsTimestamp

ClaimsHash ─► GET /object/compressed/<hash>.jpg   (GuardShare + GuardHash:
                                                     path segment must equal `hash` claim)
           ─► GET /object/imported/<assetId>.ext  (GuardShare + GuardHashOriginal:
                                                     `allowOriginal` + `assetId` claim)
           ─► renew-hash-token ─► fresh ClaimsHash (presenter must hold a valid
                                                     ClaimsTimestamp with matching timestamp)
```

The frontend service worker (`serviceWorker.ts`) injects the `Authorization:
Bearer` header for media requests from tokens persisted in IndexedDB
(`db.ts: storeAssetToken`), and `tokenStore.ts` renews both token tiers before
expiry.

### Request guards (the authorization manifest)

| Guard               | Accepts                                                                     | Used by                                                                                                                                                                                                                                                                               |
| ------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GuardAuth`         | cookie of a user with the admin role                                        | config write/export/import (incl. password change), user management (list/set-admin), album cover/assign/create-dir, delete, index jobs, rebuild, rotate, regenerate thumbnail, tags/flags/rating, share create/edit, album/tag lists, fs completion, album-index status, test probes |
| `GuardUser`         | cookie of any authenticated user (admin or not); share tokens rejected      | `PUT /put/users/password`                                                                                                                                                                                                                                                             |
| `GuardShare`        | share headers/query, or cookie of a user with the admin role                | prefetch, get-config, image serving (with `GuardHash`), **set_album_title, set_user_defined_description**                                                                                                                                                                             |
| `GuardTimestamp`    | `ClaimsTimestamp` bearer whose `timestamp` claim equals the query parameter | get-data, get-metadata, get-scroll-bar                                                                                                                                                                                                                                                |
| `GuardHash`         | `ClaimsHash` whose `hash` claim equals the URL path segment                 | `GET /object/compressed/...`                                                                                                                                                                                                                                                          |
| `GuardHashOriginal` | `ClaimsHash` with `allowOriginal` + matching `asset_id` claim               | `GET /object/imported/...`                                                                                                                                                                                                                                                            |
| `GuardUpload`       | share with `showUpload` + matching `presigned_album_id`, or admin-role user | `POST /upload`                                                                                                                                                                                                                                                                        |
| `GuardReadOnlyMode` | rejects with 405 when `readOnlyMode` is on                                  | every mutating route                                                                                                                                                                                                                                                                  |

### Configuration surface

- **`read_only_mode`** — global write kill-switch (405 on all mutations).
- **`auth_key` rotation** — changing it invalidates every outstanding token of
  all three types at once.
- **User store** — `<DATA_HOME>/auth/users.redb` is the userdb of record:
  `username ⇒ {hash?, admin, home, ver}` (PBKDF2 hash optional — a hash-less
  entry cannot log in). Every read-modify-write is one redb write
  transaction, so mutations are atomic and serial; the engine's file lock
  excludes other processes while the server holds the store open. The
  server opens the store with the first authentication, so the offline CLI
  must run while the server is stopped — a conflicting write fails loudly
  instead of corrupting the store. Identity reads, the user list and the
  enrollment emptiness all derive from this one store. The offline
  `picasu-cmd user list|add|mod|del|passwd|invalidate` administers it (see
  [Admin CLI](admin-cli.md)); hashes are never exported. Existence, role
  and token version are re-checked per request for
  `GuardAuth`/`GuardShare`/`GuardUser`, so a removal, a demotion or an
  invalidated token takes effect before token expiry.
- **Export** (`GET /get/config/export`) returns the serialized
  configuration to an admin. The signing key is redacted (`authKey` is
  removed, never exported); no plaintext secret leaves via the API.

### Auth state model

The rule: **a token is a capability, and the server remembers only what it
must.** A credential that the server keeps no state for can be checked on
its own; a credential whose server-side record exists can be listed and
revoked by id.

| Credential                                        | Carries                                        | Server state about it                            |
| ------------------------------------------------- | ---------------------------------------------- | ------------------------------------------------ |
| identity token (`Claims` with `Role::User`, 14 d) | role, `exp`, `jti`, `ver`                      | the identity record + the revocation list        |
| share token (`Claims` with `Role::Share`, 14 d)   | the share record and its album                 | the album's `shareList`                          |
| snapshot token (`ClaimsTimestamp`, 300 s)         | the share policy, the timestamp, `exp`         | none                                             |
| serving token (`ClaimsHash`, 300 s)               | the hash, the asset id, `allowOriginal`, `exp` | none                                             |
| invite token (128-bit bearer, 7 d by default)     | nothing — it _is_ the credential               | the invite record (hash-keyed, until it expires) |

**The identity token is stateless.** The server keeps no session list: a
token is accepted because it carries the role, the expiry and the user's
token version, not because the server remembers that the session exists.
The list it would replace bought nothing. An unused device is covered by
the token expiry; a suspected compromise is covered by a password change or
an explicit "revoke all sessions", both of which bump the user's version.
And a positive allowlist makes a token insufficient on its own — the `jti`
is not a secret, so anyone able to sign a token can mint one, and the
allowlist added bookkeeping rather than security.

What is left has to stay revocable, so the server keeps the _negative_ of
what the allowlist held: a revocation list, each entry carrying the expiry
of the thing it revokes.

**The snapshot and serving tokens are stateless.** They live for minutes and
are scoped to one artifact. The share policy they carry is re-read from the
album record on every request (`revalidate_share_record`), so a share that
was disabled stops yielding refreshed capabilities.

**The invite token is memorized.** An invite is a random 128-bit bearer shown
to its bearer exactly once; only its blake3 hash is stored, keyed by the
hash hex, so a leaked database row cannot be replayed as a token. Invites
are few and spend most of their life in the "used or revoked" state, so a
list of them has to be maintained either way — a revocation entry would
hold the same ids and nothing more. The record carries what the revocation
entry cannot: who created the invite, and which user redeemed it. An invite may optionally be bound to a user id at creation; redemption then requires that same id, and a mismatch is rejected without consuming the invite.

**The share token is memorized.** A share binds an album to a policy
(password, expiry, flags) and the admin page lists the shares and revokes
them by id. The share record is the thing the admin manages; a revocation
entry would only say that one is dead.

Two files hold this state, and they have different lifetimes:

| File                           | Holds                                                             | Lifetime                          |
| ------------------------------ | ----------------------------------------------------------------- | --------------------------------- |
| `<DATA_HOME>/auth/users.redb`  | the **identity record**: `username ⇒ {hash?, admin, home, ver}`   | persistent — the userdb of record |
| `<DATA_HOME>/db/index_v5.redb` | the **revocation list**, the invite records and the share records | ephemeral — runtime state         |

The guard checks a presented identity token against both:

1. the identity record — the user must exist, and the token's `ver` must be
   the user's current version;
2. the revocation list — the token's `jti` must not be revoked.

`ver` is a per-user counter in the identity record. It is bumped by every
change that must kill the outstanding tokens of that user: a password
change, a role change, an explicit invalidation (`picasu-cmd user
invalidate`). The bump happens in the same transaction as the change, so a
crash cannot leave a new password with live old tokens. The signing secret
(`auth_key`) is the global epoch: rotating it invalidates every token at
once, because no signature verifies any more.

Identity lives in exactly one place: existence, role and password come from
the user store, and the server keeps no second copy of them. A user
provisioned offline by `picasu-cmd` is therefore a user like any other — it
logs in, and it appears in the admin listing.

### User store consistency model

The user store is a file with compare-and-swap semantics, not a database
with interactive transactions. Four rules describe what it guarantees:

- **M1 — single writer, single critical section.** Every mutation runs
  inside one store read-modify-write (one redb write transaction). The
  mutation reads the state it needs inside that section; no operation
  spans two. Concurrent cooperating writers serialize.
- **M2 — last-writer-wins per field, deterministic.** Serialization
  makes concurrent same-field edits deterministic, not merged: whichever
  commits last is retained. Non-overlapping field edits converge (both
  apply). Like unix `vipw`, the store warns, it does not merge — intent
  conflicts are the operators' problem.
- **M3 — one authoritative store per fact.** Identity, hash, `admin`,
  `home` and the token version live only in the user store; revocations
  live only in the gallery index redb. An operation touching both
  (deletion: entry + revocations; invited registration: invite consumption
  - entry) executes in a fixed, crash-safe order and accepts the
    documented intermediate state.
- **M4 — durable, atomic publication.** A reader never observes a torn
  store; a crash leaves the pre- or post-state, never a partial one.
  The engine guarantees this, not application code.

Explicit **non-promises** — stated so no caller relies on them:

- No lost-update protection for two clients editing the same field from
  stale reads (that needs a version/`updated_at` pre-image; not
  implemented). Two admins editing one user: last writer wins (M2).
- No bounded staleness bound for a running server's view of the store;
  readers go through the store, but no reload signal exists to bound how
  stale a cached view may be.

The invariants derived from this model are pinned as tests where a test
can force the condition: zero-admin guard, fail-closed verification
(`hash: None`, corrupt store), concurrent-write convergence, the token
version bumps, and kill-during-write durability (a SIGKILLed writer leaves
the store at the committed state) in `backend/src/auth/user_store.rs`;
invite-redeem atomicity and the register-then-login end state in
`backend/src/auth/invites.rs` and `backend/src/tests/register.rs`. The
state-model invariants — a stale version is refused, a revoked `jti` is
refused, a password change kills every token of that user, a user
provisioned offline can log in — are pinned in
`backend/src/router/auth.rs`, `backend/src/tests/r6_session_revocation.rs`
and `backend/src/tests/auth_users.rs`.

---

## Security assessment

A security assessment of this system produced findings F1–F10. The finding
bodies live with the work that owns them, not here:

- remediated F1–F4, with transcripts: `.plan/authz-fix.md` appendix;
- open F5–F10, with transcripts and follow-up recommendations:
  `.plan/auth-hardening.md`.

| Finding                         | Severity | Status                                                                 | Owner                                 |
| ------------------------------- | -------- | ---------------------------------------------------------------------- | ------------------------------------- |
| F1 — token type confusion       | High     | fixed                                                                  | `.plan/authz-fix.md`                  |
| F2 — `get-metadata` unscoped    | Medium   | fixed                                                                  | `.plan/authz-fix.md`                  |
| F3 — share writes unbound       | High     | fixed                                                                  | `.plan/authz-fix.md`                  |
| F4 — renewal unbound            | Medium   | fixed                                                                  | `.plan/authz-fix.md`                  |
| F5 — no rate limiting/lockout   | Medium   | open                                                                   | `.plan/auth-hardening.md`             |
| F6 — open installs bind 0.0.0.0 | Medium   | open                                                                   | `.plan/auth-hardening.md`             |
| F7 — JS-readable session cookie | Medium   | fixed (R5: HttpOnly `Set-Cookie` + profile body + `POST /post/logout`) | `.plan/r5-httponly-session-cookie.md` |
| F8 — plaintext credentials      | Medium   | open                                                                   | `.plan/auth-hardening.md`             |
| F9 — read-only lift via import  | Medium   | open                                                                   | `.plan/auth-hardening.md`             |
| F10 — bounded issues            | Low      | open                                                                   | `.plan/auth-hardening.md`             |

### What holds up

The architecture's core decisions verify well under attack:

- **Algorithm pinning** — HS256-only validation with tests guarding it; no
  algorithm-confusion path (the remediated F1 issue was claim-type, not algorithm-type).
- **Snapshot binding** — `GuardTimestamp` requires the query `timestamp` to
  equal the signed claim, so a token for one snapshot cannot read another;
  cross-snapshot access did not succeed in testing.
- **Per-asset binding** — `GuardHash` compares the URL path segment against the
  signed `hash` claim; a token minted for one asset does not serve another.
- **Share state is re-read every request** — disabling or expiring a share takes
  effect immediately for identity checks (though not for already-minted capabilities — see F4 in `.plan/authz-fix.md`).
- **Share IDs are 64-char CSPRNG values** (~330 bits) and album filtering
  happens server-side in the query expression — a share cannot widen its
  snapshot by supplying its own filter.
- **No CORS headers are emitted** (verified: no `Access-Control-*` in
  responses), so cross-origin browser reads are denied by default; combined
  with SameSite=Strict this gives a credible CSRF/XSS-theft posture for the
  cookie.
- **Rocket `Shield` defaults** are attached (`nosniff`, `X-Frame-Options:
SAMEORIGIN`, `Permissions-Policy`), and `GuardReadOnlyMode` blocks mutations
  while read-only mode is on — with the import-path caveat of F9 in `.plan/auth-hardening.md`.
