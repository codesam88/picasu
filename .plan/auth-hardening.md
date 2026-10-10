---
status: open
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

Transcripts below describe the single-password instance under test. User
authentication has since replaced the single shared password (PBKDF2 store,
per-user admin role); the open findings are assumed still present — F5 and F8
carry inline updates where the new system already changed the picture, and
F6's open-installation mechanism is now an empty user store rather than an
unset password.

### Findings

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
has it on the request). (Update: user passwords are now PBKDF2 hashes verified
in constant time; throttling, lockout, and client-address logging remain open.)

#### F6 — Fresh installs are wide open, and bind to all interfaces by default (Medium, reproduced)

With `password` unset, the instance answers `GET /get/get-albums` with 200 to
an unauthenticated request and issues an admin JWT for any password submitted;
`DELETE /delete/delete-data` reached body validation (422, not 401). Meanwhile
the default `address` is `0.0.0.0`. A freshly deployed gallery is therefore
full-admin-open to the entire reachable network until someone sets a password.
The no-password convenience is reasonable for first run; the default bind is
what makes it dangerous. **Fix:** default to `127.0.0.1`, or force password
setup before mutating routes become available. (Update: the open condition is
now an empty user store rather than an unset password; the bind default and
the substance of the finding are unchanged.)

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
disclosure with no time-to-rotate window. (Update: user passwords are now
PBKDF2 hashes in a separate file, never exported; the legacy config
`password`, `auth_key`, and share passwords remain plaintext as described.)
**Fix:** store a slow hash of the
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

### Recommendations (open items)

Items 1–3 (token types, write binding, metadata/renewal scope) are implemented
— see `.plan/authz-fix.md`. The remaining items trade some first-run
convenience and a round trip of login plumbing against materially better
brute-force and XSS posture; given the self-hosted, trusted-guests goal
(`docs/auth.md`) either choice is defensible, but they should be conscious
decisions rather than defaults.

1. Throttle `/post/authenticate`, log the client address (F5; constant-time
   comparison done via PBKDF2); default-bind to localhost or force first-run
   password setup (F6).
2. Move cookie issuance server-side with HttpOnly (F7); hash the remaining
   plaintext secrets or document file permissions as load-bearing (F8);
   `Cache-Control: private` on `/object` and drop `?token=` (F10).
3. Guard `import_config` with `GuardReadOnlyMode` (F9) — one attribute; the
   deeper re-auth requirement is already decided in `bug-readonly-lockout`.
