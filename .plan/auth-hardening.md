---
status: in-progress
type: bug
priority: high
area: backend
---

## Notes

Open security findings F5–F10 from the 2026-10-04 assessment of the
authentication subsystem. Remediated findings F1–F4 live with the work that
fixed them (`.plan/authz-fix.md` appendix); the assessment's method and the
open findings live here. `docs/auth.md` keeps the mechanism and a status
table only.

### Scope and method

Static review of `backend/src/router/auth.rs`, the route modules,
`model/config.rs`, and the frontend login/interceptor/service-worker code,
plus dynamic verification against a local instance on 2026-10-04: two albums,
two shares created with `showMetadata=false`/`showDownload=false`. Findings
marked _reproduced_ were triggered end-to-end against that instance with curl;
findings marked _from source_ are static-analysis conclusions. No TLS,
reverse-proxy, browser-plugin, or dependency audit was performed. Severities
assume the self-hosted, share-guests-are-semi-trusted threat model of the
design; in a publicly exposed deployment they should be treated one level
higher.

Transcripts below are historical: they describe the retired single-password
system as tested on 2026-10-04 and are not reproducible against the current
user-based authentication. Status lines above each finding record the current
state.

### Findings

#### F5 — No rate limiting or lockout on password authentication (Medium, reproduced)

Status: fixed — POST /post/users/authenticate has 3 sliding-window buckets (IP-short, IP-long, user-long) with 429 + Retry-After; IP from transport unless trust_proxy_forwarded_for; IP logged. Bypassable by PICASU_RATE_LIMITS=false (documented fuzz switch); pre-epoch clock fails open with warn; register path unthrottled; bans time-boxed in process memory (restart clears; no permanent lockout).

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

#### F6 — Fresh installs are wide open (Medium, reproduced)

Status: fixed — closed by default (empty store → GuardAuth 401 / GuardUser 403; first admin via --bootstrap-admin TOFU flag ∧ empty store, refuse-to-start on seeded store, or picasu-cmd CLI). The 0.0.0.0 bind default is accepted: binding is operator-configurable and not a risk on a closed install.

With `password` unset, the instance answers `GET /get/get-albums` with 200 to
an unauthenticated request and issues an admin JWT for any password submitted;
`DELETE /delete/delete-data` reached body validation (422, not 401). Meanwhile
the default `address` is `0.0.0.0`. A freshly deployed gallery is therefore
full-admin-open to the entire reachable network until someone sets a password.
The no-password convenience is reasonable for first run; the default bind is
what makes it dangerous. **Fix:** default to `127.0.0.1`, or force password
setup before mutating routes become available.

#### F7 — Admin session cookie is readable by JavaScript (Medium, from source)

Status: fixed — server HttpOnly/Secure-if-https/SameSite=Strict Set-Cookie, profile body, POST /post/logout; no session token in bodies.

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

Status: fixed — user and share passwords are argon2id PHC strings at rest
(OWASP minimums m=19456 KiB, t=2, p=1; `PICASU_WEAK_PWHASH=1` selects
trivial cost for test runs and never belongs in production). Shares keep
the decided no-migration stance: legacy plaintext rows verify verbatim
until recreated or re-set. Users dual-path verify and rehash legacy
PBKDF2 rows in place on the next successful login (no token-version
bump). auth_key necessarily readable, owner-only 0600 test-pinned.

`password` and `auth_key` sit in `config.toml` as plaintext (verified on disk),
share passwords are stored plaintext in the album records, and passwords are
compared verbatim. The export endpoint returns them in plaintext to any admin
(by design, but it multiplies the value of an admin token). There is no hashing,
so compromise of the config file or a backup is immediate and total credential
disclosure with no time-to-rotate window.
**Fix:** store a slow hash of the
password (the comparison input is small); keep `auth_key` as-is but document
that config-file permissions are load-bearing.

#### F9 — Read-only mode can be lifted through config import (Medium, reproduced)

Status: fixed — POST /post/config/import deleted; only writers of read_only_mode are PUT /put/admin/config (read-only-gated) and POST /post/admin/config/disable-read-only (GuardAuth only).

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

Status: fixed — config disclosure split by role + original-denial 401 fixed
earlier; `extract_bearer_token` is header-only since 2026-10-27 (the query
form leaked credentials into proxy logs, history and `Referer`); `/object`
serves `Cache-Control: private` (kept `public` only for the genuinely public
`/assets` and `/favicon.ico`), with a code comment noting a future public
gallery mode would revisit that. URL-passable credentials keep their own
shapes and were not affected: share links are a path slug plus the
`x-album-id`/`x-share-id` pair, invite links carry `?invite=` in the
frontend route and redeem via POST body.

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

### Recommendations (open items)

Items 1–3 (token types, write binding, metadata/renewal scope) are implemented
— see `.plan/authz-fix.md`. The remaining items trade some first-run
convenience and a round trip of login plumbing against materially better
brute-force and XSS posture; given the self-hosted, trusted-guests goal
(`docs/auth.md`) either choice is defensible, but they should be conscious
decisions rather than defaults.

Resolved:

1. F5 done (buckets + 429 + IP logging); F6 done (closed by default, bind
   accepted as configurable); F7 done (server HttpOnly cookie).
2. F8 done (2026-10-27): argon2id for both password stores — shares keep
   legacy plaintext rows until recreated, users dual-path verify and
   upgrade on next login; `auth_key` permissions load-bearing. F10 done
   (2026-10-27): header-only bearer, `Cache-Control: private` on
   `/object`, invite/share URL shapes untouched.
3. F9 done by deleting the import endpoint; the read-only lift is the
   dedicated `POST /post/admin/config/disable-read-only` route.
