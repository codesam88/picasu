# Authentication and Authorization

This document describes Picasu's authentication system: its goals, the paradigm
it follows, and the detailed mechanism as implemented in
`backend/src/router/auth.rs` and the route modules. The desired properties —
the model from which enforcement rules and tests are derived — live in
[`authz-model.md`](authz-model.md). The final section is a
security assessment written from an external reviewer's perspective. The
assessment combines source review with dynamic verification against a local
test instance (two albums, two shares, password configured); each finding is
marked _reproduced_ or _from source_ to say which.

## Goals

Picasu is a self-hosted gallery with a deliberately small identity model. The
design goals, in priority order:

1. **One admin, many guests.** There is no user table. A single shared admin
   password guards management operations; "users" beyond the admin exist only as
   **share links** — capability URLs that expose one album under an explicit
   policy.
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

| Secret       | Source                                                 | Role                                                                              |
| ------------ | ------------------------------------------------------ | --------------------------------------------------------------------------------- |
| `password`   | `[secrets]` in `config.toml`                           | admin login credential, compared verbatim (after trim)                            |
| `auth_key`   | `[secrets]` in `config.toml`, or env `PICASU_AUTH_KEY` | HS256 key for **all** JWT types                                                   |
| fallback key | 32 random bytes, generated once per process            | used only when `auth_key` is unset; a restart invalidates every outstanding token |

`AppConfig::get_jwt_secret_key` (`model/config.rs`) selects the key. All tokens
are HS256-encoded with it; validation pins the algorithm list to `[HS256]`
(`VALIDATION` in `router/auth.rs`, enforced by unit tests) so algorithm-confusion
attacks are excluded by construction.

### Admin sign-in

1. `POST /post/authenticate` takes the password as a JSON string, trims it, and
   compares it to the configured password. A match returns a 14-day
   `Claims { role: Admin, exp }` JWT in the response body.
2. The **client** stores it in a cookie named `jwt` via js-cookie
   (`LoginPage.vue`): `httpOnly: false`, `secure: true`, `sameSite: Strict`,
   `expires: 14` days. The server never issues `Set-Cookie`.
3. `GuardAuth` reads the cookie, decodes it with `VALIDATION` (expiry enforced),
   and requires `role == Admin`.
4. Logout (`GalleryBar.vue`) removes the cookie client-side only; the token
   remains cryptographically valid until expiry or until `auth_key` is rotated.

**No-password mode:** if `password` is unset, `try_jwt_cookie_auth` returns an
admin `Claims` value without reading any credential, and `authenticate` accepts
any input. A fresh install is therefore fully open until a password is set —
this is the documented first-run flow, but see Finding F6 for its interaction
with the default bind address.

### The three token types

| Type              | Claims                                                               | TTL   | Minted by                        | Consumed by                                                |
| ----------------- | -------------------------------------------------------------------- | ----- | -------------------------------- | ---------------------------------------------------------- |
| `Claims`          | `role` (Admin / Share(resolved)), `exp`, `typ: admin`                | 14 d  | login, share resolution          | `GuardAuth`, `GuardShare` (cookie fallback), `GuardUpload` |
| `ClaimsTimestamp` | `resolvedShareOpt`, `timestamp`, `exp`, `typ: snapshot`              | 300 s | `POST /get/prefetch`             | `GuardTimestamp`, `TimestampGuardModified`                 |
| `ClaimsHash`      | `allowOriginal`, `hash`, `assetId`, `timestamp`, `exp`, `typ: asset` | 300 s | every row of `GET /get/get-data` | `GuardHash`, `GuardHashOriginal`                           |

All three are plain JWTs signed with the same `auth_key`. Each carries a `typ`
claim and is decoded through a typed entry point that rejects a mismatched
`typ`; the claims structs also set `deny_unknown_fields`, so the three
cross-type decode paths fail closed (see Finding F1, now fixed).

### Share resolution

`GuardShare` tries three sources in order:

1. **Headers** `x-album-id` + `x-share-id` (both or neither; half-supplied is a
   400), plus `x-share-password` when the share has one.
2. **Query parameters** `albumId` + `shareId`.
3. **Admin cookie fallback** — a logged-in admin passes any share-guarded route.

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

| Guard               | Accepts                                                                     | Used by                                                                                                                                                                                                                                             |
| ------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GuardAuth`         | admin cookie (or no-password mode)                                          | config write/export/import (incl. password change), album cover/assign/create-dir, delete, index jobs, rebuild, rotate, regenerate thumbnail, tags/flags/rating, share create/edit, album/tag lists, fs completion, album-index status, test probes |
| `GuardShare`        | share headers/query, or admin cookie                                        | prefetch, get-config, image serving (with `GuardHash`), **set_album_title, set_user_defined_description**                                                                                                                                           |
| `GuardTimestamp`    | `ClaimsTimestamp` bearer whose `timestamp` claim equals the query parameter | get-data, get-metadata, get-scroll-bar                                                                                                                                                                                                              |
| `GuardHash`         | `ClaimsHash` whose `hash` claim equals the URL path segment                 | `GET /object/compressed/...`                                                                                                                                                                                                                        |
| `GuardHashOriginal` | `ClaimsHash` with `allowOriginal` + matching `asset_id` claim               | `GET /object/imported/...`                                                                                                                                                                                                                          |
| `GuardUpload`       | share with `showUpload` + matching `presigned_album_id`, or admin           | `POST /upload`                                                                                                                                                                                                                                      |
| `GuardReadOnlyMode` | rejects with 405 when `readOnlyMode` is on                                  | every mutating route                                                                                                                                                                                                                                |

### Configuration surface

- **`read_only_mode`** — global write kill-switch (405 on all mutations).
- **`auth_key` rotation** — changing it invalidates every outstanding token of
  all three types at once; this is the only revocation mechanism that exists.
- **Password change** — does **not** touch `auth_key`, so existing admin tokens
  survive a password change (documented in `update_password_handler`).
- **Export** (`GET /get/config/export`) returns the full config including
  `password` and `authKey` in plaintext to an admin, by design.

---

## Security assessment

**Scope and method.** Static review of `backend/src/router/auth.rs`, the route
modules, `model/config.rs`, and the frontend login/interceptor/service-worker
code, plus dynamic verification against a local instance on 2026-10-04: two
albums, two shares created with `showMetadata=false`/`showDownload=false`.
Findings marked _reproduced_ were triggered end-to-end against that instance
with curl; findings marked _from source_ are static-analysis conclusions. No
TLS, reverse-proxy, browser-plugin, or dependency audit was performed.
Severities assume the self-hosted, share-guests-are-semi-trusted threat model of
the design; in a publicly exposed deployment F1–F4 should be treated one level
higher.

### Findings

#### F1 — JWT claim type confusion lets any share bypass its own policy (High, reproduced — fixed)

`ClaimsHash` and `ClaimsTimestamp` are indistinguishable to the decoder: both
are HS256 tokens under the same key, neither carries a `typ` claim, and serde
ignores the extra fields while defaulting the missing `resolvedShareOpt` to
`None` (which `resolve_show_download_and_metadata` treats as full access). A
share viewer already receives `ClaimsHash` tokens in every `get-data` row, so
the attack uses only tokens the share itself is legitimately given:

1. **Metadata hiding bypass.** Calling `GET /get/metadata/<id>` with a
   `ClaimsHash` as bearer instead of the timestamp token returned the full
   `path` object (`/tmp/.../images/albumA/a.jpg`) where the legitimate token
   correctly returned `path: null` for the same `showMetadata=false` share.
2. **Download-hiding bypass.** Calling `GET /get/get-data` with the same
   confused bearer made the server mint fresh tokens with `allowOriginal: true`
   for a `showDownload=false` share; that token then fetched
   `GET /object/imported/...` — HTTP 200, bytes identical to the original file
   (the legitimate token never produces `allowOriginal=true`).
3. **Unlimited renewal.** `POST /post/renew-timestamp-token` accepts a
   `ClaimsHash` as the submitted body token and re-issues it as a
   `ClaimsTimestamp` with `resolvedShareOpt: null`, converting a 300-second
   asset token into a renewable full-policy token.

Snapshot scoping still holds (rows remain limited to the share's album), but
every policy flag a share is supposed to enforce — metadata hiding, download
hiding — is bypassable by the share itself. **Fix:** add a `typ` claim
(`admin` / `snapshot` / `asset`) to each token type and reject mismatches at
decode; optionally decode with `deny_unknown_fields`.

#### F2 — `get-metadata` is not scoped to the share's album (Medium, reproduced — fixed)

A share on AlbumB holding a valid `ClaimsTimestamp` fetched AlbumA's asset by
ID and received its full metadata (path included). Authorization on this route
is effectively "knows a 64-character asset ID" rather than "is entitled to this
album". Asset IDs are high-entropy, but they appear in URLs, logs, and referrer
headers; once known, any share (subject to its own `showMetadata` flag) can
read any asset's metadata. **Fix:** resolve the token's `resolvedShareOpt` and
verify the requested `asset_id` belongs to that album before composing the
record.

#### F3 — Share-guarded write endpoints do not bind the caller to the target (High, reproduced — fixed)

Two routes accept `GuardShare` but then take their target from the request body
without comparing it to the authenticated share's album:

- `PUT /put/set_album_title` — AlbumB's share renamed **AlbumA** (HTTP 200;
  verified by re-reading the album list as admin). Album IDs are visible in
  share URLs (`/share/<albumId>-<shareId>`), so they are not secret between
  guests.
- `PUT /put/set_user_defined_description` — AlbumA's share wrote a description
  into an asset of AlbumB by supplying AlbumB's snapshot `timestamp` (HTTP 200,
  write verified via metadata read). Snapshot timestamps are wall-clock
  milliseconds; combined with the absence of rate limiting (F5), blind
  enumeration of a time window is practical.

Both are integrity violations by any share guest against any album. (Related
design note, documented in the route itself: description writes do not consult
`showMetadata` at all.) **Fix:** derive the target album from
`auth.claims.get_share()` and reject any body `albumId`/snapshot that does not
belong to it.

#### F4 — Token renewal is not bound to the presenter's share (Medium, reproduced — fixed)

`POST /post/renew-timestamp-token` validates that the _presenter_ holds any
valid share (`GuardShare`) and that the submitted token is
signature-valid-but-possibly-expired — but never checks that the submitted
token's embedded share equals the presenter's. Verified: share1's credentials
successfully renewed share2's timestamp token, which was re-issued with
share2's album binding intact. Consequences:

- A leaked timestamp token can be kept alive indefinitely by anyone holding any
  other valid share credential; expiry is only a speed bump.
- Renewal re-issues the embedded share claims **without re-running
  `validate_share_access`**, so a share that has since been disabled, expired,
  or had its password changed continues to yield refreshed tokens as long as
  the token was minted beforehand.

`renew-hash-token` is better bound (presenter must hold a `ClaimsTimestamp`
whose `timestamp` claim matches) but inherits the same missing re-validation on
repeated cycles. **Fix:** compare the submitted token's share against the
presenter's, and re-resolve + re-validate the embedded share at renewal.

#### F5 — No rate limiting or lockout on password authentication (Medium, reproduced)

Sixteen consecutive wrong-password attempts on `/post/authenticate` each
returned 401 with no backoff, no 429, and no lockout. Failures are logged —
one `API Error: Authentication Error: Invalid password` record per attempt
(verified: 16 records for 16 attempts) — but **without the client address**,
which limits forensic use, and the password comparison is a non-constant-time
`==` on `String`. For a
single-password system exposed to a network, online guessing is the primary
attack and nothing throttles it. **Fix:** per-source attempt throttling,
constant-time comparison, include the remote address in failure logs (Rocket
has it on the request).

#### F6 — Fresh installs are wide open, and bind to all interfaces by default (Medium, reproduced)

With `password` unset, the instance answers `GET /get/get-albums` with 200 to
an unauthenticated request and issues an admin JWT for any password submitted;
`DELETE /delete/delete-data` reached body validation (422, not 401). Meanwhile
the default `address` is `0.0.0.0`. A freshly deployed gallery is therefore
full-admin-open to the entire reachable network until someone sets a password.
The no-password convenience is reasonable for first run; the default bind is
what makes it dangerous. **Fix:** default to `127.0.0.1`, or force password
setup before mutating routes become available.

#### F7 — Admin session cookie is readable by JavaScript (Medium, from source)

The JWT cookie is written client-side with `httpOnly: false` — the code comment
in `LoginPage.vue` itself notes it "should be true". The SPA renders
attacker-influenced strings (descriptions, tags, filenames); no `v-html` sink
was found in the frontend during this review, but with a readable cookie any
future XSS exfiltrates a 14-day admin token, and HttpOnly is the standard
backstop for exactly that case. Related: because the server never sets the
cookie, HttpOnly cannot be
achieved without moving issuance server-side; `secure: true` on a default
plain-HTTP deployment also means browsers treat the flag inconsistently outside
localhost (not systematically tested here). SameSite=Strict is correctly set
and is the current CSRF defense — there are no CSRF tokens. **Fix:** issue the
cookie via `Set-Cookie` with `HttpOnly; Secure; SameSite=Strict` instead of
returning the raw token in the body.

#### F8 — Credentials stored and transported in plaintext (Medium, from source)

`password` and `auth_key` sit in `config.toml` as plaintext (verified on disk),
share passwords are stored plaintext in the album records, and passwords are
compared verbatim. The export endpoint returns them in plaintext to any admin
(by design, but it multiplies the value of an admin token). There is no hashing,
so compromise of the config file or a backup is immediate and total credential
disclosure with no time-to-rotate window. **Fix:** store a slow hash of the
password (the comparison input is small); keep `auth_key` as-is but document
that config-file permissions are load-bearing.

#### F9 — Read-only mode can be lifted through config import (Medium, reproduced)

Read-only mode is meant to be a kill-switch that a bare token cannot disable:
`PUT /put/config` carries `GuardReadOnlyMode`, so while the mode is on, the
endpoint that would turn it off answers 405 (verified; this is also the premise
of the open `bug-readonly-lockout` plan, which decides that lifting the mode
must require password re-authentication, not a token). But
`POST /post/config/import` takes only `GuardAuth` and replaces the whole
`AppConfig` — including `readOnlyMode` — via `AppConfig::update`, with no
read-only guard. Verified end-to-end: with the mode on, mutations returned 405
and `PUT /put/config` was refused with 405, then a single
`POST /post/config/import` carrying `"readOnlyMode": false` returned 200 and
mutations were accepted again. A captured admin token (see F7) is therefore
sufficient to disable the kill-switch before using it, which defeats the
re-authentication decision recorded in the plan. **Fix:** attach
`GuardReadOnlyMode` (and, per the plan, the future re-auth guard) to
`import_config` as well.

#### F10 — Bounded issues (Low)

- **Authenticated media responses are marked `Cache-Control: public,
max-age=31536000`** (`router/cache.rs` applies it to `/object` on any 2xx).
  `public` explicitly permits _shared_ caches to store responses to
  authorized requests (RFC 9111), so a caching reverse proxy in front of Picasu
  could retain and re-serve media to a later unauthenticated request for the
  same URL. **Fix:** `private` for `/object`.
- **`extract_bearer_token` accepts `?token=` in the URL query string.**
  Tokens in URLs leak into proxy logs, browser history, and `Referer` headers.
  The current frontend uses the `Authorization` header exclusively; the query
  path appears unused. **Fix:** remove it.
- **`GET /get/config` discloses server internals to any share holder** —
  verified response includes bind `address`/`port` and absolute `imagePath`.
- **Original-file denial returns the wrong status.** With `allowOriginal=false`
  the guard `Forward`s, so the request falls through to the SPA catch-all
  instead of receiving 401 (observed: 500 on an instance without a web root; a
  configured instance returns 200 + `index.html`). No file bytes leak — the
  denial works — but monitoring and clients see a misleading status.

### What holds up

The architecture's core decisions verify well under attack:

- **Algorithm pinning** — HS256-only validation with tests guarding it; no
  algorithm-confusion path (F-class issues above are claim-type, not
  algorithm-type).
- **Snapshot binding** — `GuardTimestamp` requires the query `timestamp` to
  equal the signed claim, so a token for one snapshot cannot read another;
  cross-snapshot access did not succeed in testing.
- **Per-asset binding** — `GuardHash` compares the URL path segment against the
  signed `hash` claim; a token minted for one asset does not serve another.
- **Share state is re-read every request** — disabling or expiring a share takes
  effect immediately for identity checks (though not for already-minted
  capabilities; see F4).
- **Share IDs are 64-char CSPRNG values** (~330 bits) and album filtering
  happens server-side in the query expression — a share cannot widen its
  snapshot by supplying its own filter.
- **No CORS headers are emitted** (verified: no `Access-Control-*` in
  responses), so cross-origin browser reads are denied by default; combined
  with SameSite=Strict this gives a credible CSRF/XSS-theft posture for the
  cookie.
- **Rocket `Shield` defaults** are attached (`nosniff`, `X-Frame-Options:
SAMEORIGIN`, `Permissions-Policy`), and `GuardReadOnlyMode` blocks mutations
  while read-only mode is on — with the import-path caveat of F9.

### Recommendations, prioritized

1. ~~Add `typ` claims and reject cross-type decoding (F1)~~ — done: each claims
   type carries a `typ` claim and is decoded through `decode_typed`, with
   `deny_unknown_fields` on the claims structs.
2. ~~Bind the two share-guarded write routes to the caller's album (F3)~~ —
   done: `set_album_title` and `set_user_defined_description` derive the target
   from `GuardShare::claims`.
3. ~~Album-scope `get-metadata` (F2) and bind renewal to the presenter's share
   with share re-validation (F4)~~ — done: `get-metadata` 404s on an asset
   outside the share's album; renewal requires presenter share == token share
   and re-validates the embedded share from the DB.
4. Throttle `/post/authenticate`, compare in constant time, log the client
   address (F5); default-bind to localhost or force first-run password setup
   (F6).
5. Move cookie issuance server-side with HttpOnly (F7); hash the stored
   password (F8); `Cache-Control: private` on `/object` and drop `?token=`
   (F10).
6. Guard `import_config` with `GuardReadOnlyMode` (F9) — one attribute; the
   deeper re-auth requirement is already decided in `bug-readonly-lockout`.

Items 1–3 were correctness fixes with no design trade-offs and are implemented
(the reproductions are pinned by `backend/src/tests/authz.rs`). Items 4–6 trade
some first-run convenience and a round trip of login plumbing against
materially better brute-force and XSS posture; given goal 5 (self-hosted,
trusted guests) either choice is defensible, but they should be conscious
decisions rather than defaults.
