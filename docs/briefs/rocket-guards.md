# Rocket guards and capability tokens

Picasu's authorization is declared as Rocket request guards (`FromRequest`
impls), one per route. The guard list on a handler plus its error
propagation _is_ the auditable authorization manifest of the API. This
brief covers the mechanics of crafting new guards and routes without
breaking the model. Policy (who may do what) lives in
[`authz-model.md`](../authz-model.md); mechanism status in
[`auth.md`](../auth.md).

## Baseline (the obvious third)

Three pieces: JWT claims types carrying a `typ` discriminator, one
`VALIDATION` pinning HS256, and guard structs resolving identity or
capability per request.

**Token types** (`backend/src/router/auth.rs`): identity (`Claims`,
`typ: admin`, 14-day TTL, minted by `authenticate` login/migration),
snapshot (`ClaimsTimestamp`, `typ: snapshot`, 5-minute TTL, minted in
`router/get/get_prefetch.rs` from the caller's share scope), asset
(`ClaimsHash`, `typ: asset`, per-file serving tokens). All HS256 with
one config secret (`auth_key`, fallback: 32 random bytes per process).

**Guard catalog**: `GuardAuth` (admin-role user only), `GuardUser`
(any live user, share tokens rejected — self-service routes),
`GuardShare` (live share from headers/query, else admin-cookie
fallback), `GuardTimestamp` (snapshot bearer + `?timestamp` binding),
`GuardHash`/`GuardHashOriginal` (asset bearer + path-segment
binding), `GuardUpload` (upload-share or admin), `GuardReadOnlyMode`
(405 when read-only). Every guarded handler takes
`GuardResult<T> = Result<T, AppError>` and propagates with `?`.

**Route conventions**: `#[utoipa::path]` with every response status
declared (401 uses the shared `Unauthorized` response), `AppResult`
return, `AppError::new(ErrorKind::…)` errors, CPU-heavy work
(password hashing) on `spawn_blocking`, then `just openapi-gen` and a
green `just openapi-check`. Tests use `make_client()` +
`reset_backend_state()` under `TEST_SERIAL_GUARD`
(`backend/src/tests/bootstrap.rs`).

## Non-obvious behaviors that bite

1. **Guards succeed with no credential in open states.** Empty store
   (+ the planned bootstrap flag) mints an ephemeral admin inside
   `try_jwt_cookie_auth`/`GuardUser` before any cookie is read.
   Routes that must stay public in every state (`authenticate`,
   bootstrap `users/create`, registration) take **no guard at all**;
   routes that must stay closed take one and propagate it.
2. **`GuardAuth` is admin-only, not "logged in".** Non-admin users
   pass only `GuardUser` routes — today that is password
   self-service and little else. Reaching for `GuardAuth` on a read
   is correct only if non-admins truly must not see it.
3. **A discarded rejection enforces nothing.** `let _ = auth;`
   compiles and leaves the route open while the OpenAPI document
   still declares 401 — a proven incident
   (`.plan/bug-get-rows-auth-guard-discarded.md`), now
   machine-checked per `.plan/authz-check.md`. Always `auth?` (or the
   `let _ = read_only?;` form) before touching anything.
4. **Derive targets from guard claims, not the request body.**
   `GuardShare`/`GuardTimestamp` handlers must read the operation's
   target (album, scope) from the guard's resolved claims; a
   body-supplied id is a cross-share acting bug (F2–F4).
5. **One decode entry point per type.** Always
   `decode_typed::<ExactClaims>(token, &VALIDATION)`; the `typ`
   marker plus `deny_unknown_fields` is the cross-type replay
   defense. New token types need both, plus an `authz-check`
   decode-agreement entry.
6. **Admin liveness order matters.** `GuardAuth` checks the
   token-embedded admin flag _first_, then re-reads the user record:
   demotion takes effect immediately, promotion requires re-login.
   New admin gates must preserve this order, not just the record
   read.
7. **`GuardShare` resolution order is load-bearing**: headers, then
   query, then admin-cookie fallback — and share-resolution errors
   carry their own status via `err.http_status()`, while the cookie
   fallback only ever 401s. Don't collapse them into one message.
8. **Unknown users pay a KDF round.** `login_sync` runs a dummy
   verify so wrong-password and unknown-user are timing-equal. New
   credential checks must preserve this (and never compare secrets
   with `==` — F5).
9. **Passwords are trim-canonicalized.** Creation, login, and
   migration all `trim()`; write-then-login consistency depends on
   every writer trimming. Minimum length (12) applies to _new_
   passwords only — legacy migration must accept short secrets or
   old deploys lock out.
10. **Identity tokens travel HttpOnly.** `POST /post/authenticate` issues
    the session JWT via `Set-Cookie` (`HttpOnly; SameSite=Strict; Path=/`,
    `Secure` iff `X-Forwarded-Proto: https`) and returns only the
    `{ userId, admin }` profile in the body; `POST /post/logout` clears it.
    New login-like routes must follow the same shape — never return a raw
    session token in the body, never let page JS write the session cookie.
11. **Config fields are API surface.** `AppConfig` is `ToSchema` and
    served over `GET /get/config`; a new key appears in
    `openapi.json` (regen, don't hand-edit) and needs an explicit
    PUT-allowlist decision in `router/put/edit_config.rs`.
12. **Never-created redb tables read as empty.** `get_user` relies on
    this; new tables (`INVITES`) follow the `USERS`
    `TableDefinition<&str, &str>` + JSON-value pattern in
    `storage/db.rs`, and multi-table writes share one redb
    transaction (passwd.json stays file-atomic outside it — crash
    ordering is documented per call site, not solved).

## In-depth sources (one line each)

- `docs/authz-model.md` — the formal model guards implement; read
  before adding a guard.
- `docs/auth.md` — mechanism reference (token table, sign-in steps,
  open first-run semantics).
- `.plan/auth-hardening.md` — F1–F10: every past guard/token bug,
  each with mechanism and fix shape.
- `.plan/bug-get-rows-auth-guard-discarded.md` — the discarded-guard
  incident; why propagation is checked mechanically.
- `.plan/authz-check.md` — the planned lint family (parity,
  propagation, consumption, decode agreement); new routes must
  satisfy all four by construction until the lints exist.
- `backend/src/router/post/users.rs` (`create_user_handler`) — the
  reference admin-or-bootstrap handler: guard match arms, forced
  flags, `spawn_blocking` structure.
- `backend/src/router/put/users_admin.rs` — minimal admin-only
  handler shape to copy.
- `backend/src/router/post/authenticate.rs` — public
  (guard-less) route shape, legacy-migration locking
  (`BOOTSTRAP_LOCK`: redb + passwd-file writes can't share a
  transaction, so process locks serialize them).

## Failure modes and limits

Guards are request-local: no cross-request state, no revocation list
— a stolen token lives till expiry or `auth_key` rotation. No rate
limiting on any credential endpoint (F5, open). Algorithm confusion
is excluded by the pinned list (unit-tested), but a leaked `auth_key`
forges every token type: key file permissions are load-bearing (F8).
Share-password comparison and KDF dummy-round are the only
constant-time paths; audit any new secret comparison.

## Mental model and vocabulary

Tiered capabilities: long-lived identity mints short-lived snapshot,
snapshot mints per-asset serving tokens; each tier narrows scope and
TTL so leaks have small blast radii. Guards are the enforcement
points, `GuardResult` + `?` is the manifest, `openapi.json` is its
projection. When adding a route, the review question is always: which
guard, what does its rejection do, and where does the handler's
target come from.
