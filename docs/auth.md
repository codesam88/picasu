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

| Secret       | Source                                                 | Role                                                                                           |
| ------------ | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------- |
| `password`   | `[secrets]` in `config.toml` (legacy)                  | bootstrap-only: migrates onto user `admin` at first legacy login; ignored once any user exists |
| `auth_key`   | `[secrets]` in `config.toml`, or env `PICASU_AUTH_KEY` | HS256 key for **all** JWT types                                                                |
| fallback key | 32 random bytes, generated once per process            | used only when `auth_key` is unset; a restart invalidates every outstanding token              |

`AppConfig::get_jwt_secret_key` (`model/config.rs`) selects the key. All tokens
are HS256-encoded with it; validation pins the algorithm list to `[HS256]`
(`VALIDATION` in `router/auth.rs`, enforced by unit tests) so algorithm-confusion
attacks are excluded by construction.

### Sign-in

1. `POST /post/authenticate` takes `{ userId, password }`, trims the password,
   and verifies it against the PBKDF2 password store
   (`<DATA_HOME>/auth/passwd.json`) for that user id. A match mints a random
   `jti` (UUID v4), stores it as a live session for that user, and sets a
   14-day `Claims { role: User { id, admin }, exp, jti }` JWT in an HttpOnly
   session cookie, returning the session profile `{ userId, admin }` in the
   body. Unknown users and wrong passwords both answer 401 with no
   distinguishing detail; the unknown-user path still pays one KDF round so
   timing leaks nothing.
2. The **server** issues the session cookie via `Set-Cookie`
   (`router/post/authenticate.rs`): `jwt=...; HttpOnly; SameSite=Strict;
Path=/; Max-Age=14d`, plus `Secure` iff the request arrived via
   `X-Forwarded-Proto: https` (a TLS-terminating proxy must forward that
   header; plain-HTTP LAN deploys get a non-Secure cookie). Page JS never
   sees the token, so a future XSS cannot exfiltrate the session (R5). The
   client keeps only the `{ userId, admin }` profile from the login body in
   its session store for display.
3. `GuardAuth` (and `GuardUser`) read the cookie, decode it with `VALIDATION`
   (expiry enforced), and require the `jti` to be live for that user — the
   user record is re-read on every request, so demotion or session revocation
   takes effect before token expiry. Admin routes additionally require the
   admin role from the record (promotion requires re-login; demotion takes
   effect here). Tokens without a `jti` are rejected (hard cut — pre-`jti`
   tokens do not decode).
4. Logout is `POST /post/logout`, which decodes the presented `jwt` cookie
   with the same validation the guards use; on success it removes that `jti`
   from the caller's live sessions (absent or unknown `jti` is ignored), then
   clears the cookie server-side (HttpOnly cookies cannot be removed by page
   JS) and answers 204. Missing, undecodable, or expired tokens skip removal
   but still clear the cookie and still answer 204 — idempotent with or
   without a live session. Only the presented device is logged out; other
   sessions survive. The client also clears its session profile and capability
   tokens unconditionally, so logout sticks even when the round trip fails.
   Password change (`PUT /put/users/password`, self-change and admin reset
   alike) wipes all `jti`s for the target user before writing the new hash —
   sessions first, password second, so a crash between leaves
   re-login-with-old-password, the safe direction — and therefore logs every
   device out. `GET /get/sessions` (any authenticated user via `GuardUser`)
   returns the caller's live `SessionRecord { jti, issuedAt }` array; it never
   returns another user's sessions, and an unknown post-guard id maps to 401.
5. Users are managed through `GET /get/users`,
   `PUT /put/users/password`, and `PUT /put/users/admin` (admin-only,
   except unauthenticated creation of the first user, which is forced admin).

**Login throttling (brute-force resistance):** `POST /post/authenticate`
checks three sliding-window buckets before running the password KDF, so a
throttled request stays cheap: an IP-short burst bucket (3 failures per
10 min, first ban 10 s, doubling), an IP-long bucket per client address
(16 failures per 24 h, first ban 30 min), and a user-long bucket per
claimed username (18 failures per 24 h, first ban 30 min, capped at 2 h).
A request whose ban is still active is rejected with 429 and a
`Retry-After` header (seconds; the max remaining ban across all three
buckets), distinct from the 401 of a bad credential. Buckets are keyed on
claimed usernames whether or not the account exists, so throttling
behaves identically for unknown names and reveals nothing about which
users are real. A successful login drops the username entry and clears the IP burst bucket (the IP-long history is kept). All
thresholds are knobs under `[server]` (see
[Configuration](config.md) for the full table), the client address honors
`trust_proxy_forwarded_for`, and the env-only switch
`PICASU_RATE_LIMITS=false` bypasses all login throttling (e.g. for
schema fuzzing). The login page renders the 429 as a countdown banner
and re-enables submission when the ban expires.

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
empty user store and no legacy password, every guarded route answers 401 and
`authenticate` accepts nothing. Online enrollment of the first administrator
opens only while the `--bootstrap-admin` flag (or `PICASU_BOOTSTRAP_ADMIN`,
or the `bootstrap_admin` config key) is set **and** the store is empty and no
legacy password is configured: `try_jwt_cookie_auth` then returns an admin
identity without reading a credential and `authenticate` accepts any
parseable input, until the first user is created. The flag is WARN-logged at
boot, the server refuses to start when the flag is set against an existing
`<DATA_HOME>/auth/passwd.json` (bootstrap already happened, or the store
needs repair — provision via the CLI instead), and a configured legacy
password always wins over the flag. This closes Finding F6 for the default
path; a network-exposed install is only ever open if the operator opted in.

**Legacy bootstrap:** a bare JSON string body to `authenticate` is accepted
only while the user store is empty; it verifies the legacy config `password`
and migrates it onto user `admin`. The same first-login bootstrap is
available through the object path: the first login with the legacy password
registers the claimed `userId` as the first admin. The legacy config password is ignored once
any user exists, and pre-migration identity tokens do not decode under the new
`Role` shape.

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

| Guard               | Accepts                                                                     | Used by                                                                                                                                                                                                                                                                                      |
| ------------------- | --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GuardAuth`         | cookie of a user with the admin role (or open first-run mode)               | config write/export/import (incl. password change), user management (create/list/set-admin), album cover/assign/create-dir, delete, index jobs, rebuild, rotate, regenerate thumbnail, tags/flags/rating, share create/edit, album/tag lists, fs completion, album-index status, test probes |
| `GuardUser`         | cookie of any authenticated user (admin or not); share tokens rejected      | `PUT /put/users/password`                                                                                                                                                                                                                                                                    |
| `GuardShare`        | share headers/query, or cookie of a user with the admin role                | prefetch, get-config, image serving (with `GuardHash`), **set_album_title, set_user_defined_description**                                                                                                                                                                                    |
| `GuardTimestamp`    | `ClaimsTimestamp` bearer whose `timestamp` claim equals the query parameter | get-data, get-metadata, get-scroll-bar                                                                                                                                                                                                                                                       |
| `GuardHash`         | `ClaimsHash` whose `hash` claim equals the URL path segment                 | `GET /object/compressed/...`                                                                                                                                                                                                                                                                 |
| `GuardHashOriginal` | `ClaimsHash` with `allowOriginal` + matching `asset_id` claim               | `GET /object/imported/...`                                                                                                                                                                                                                                                                   |
| `GuardUpload`       | share with `showUpload` + matching `presigned_album_id`, or admin-role user | `POST /upload`                                                                                                                                                                                                                                                                               |
| `GuardReadOnlyMode` | rejects with 405 when `readOnlyMode` is on                                  | every mutating route                                                                                                                                                                                                                                                                         |

### Configuration surface

- **`read_only_mode`** — global write kill-switch (405 on all mutations).
- **`auth_key` rotation** — changing it invalidates every outstanding token of
  all three types at once.
- **User passwords** live as PBKDF2 hashes in `<DATA_HOME>/auth/passwd.json`
  (separate from config and the database) with role records in the `users`
  table; hashes are never exported. Demotion/removal is re-checked per request
  for `GuardAuth`/`GuardShare`/`GuardUser`, so it takes effect before token
  expiry; promotion requires re-login. A password change does **not** touch
  `auth_key`, so outstanding tokens survive it.
- **Export** (`GET /get/config/export`) returns the full config including the
  legacy `password` and `authKey` in plaintext to an admin, by design. The
  legacy config password is ignored once any user exists.

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
