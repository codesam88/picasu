# Authentication Model

This document is the model of the authentication and authorization properties
Picasu intends to hold. It is the source from which the enforcement rules
(`utils/openapi-sanity`, `.plan/authz-check.md`) and the security tests
(`backend/src/tests/authz.rs`) are derived. `docs/auth.md` describes how the
mechanism works today; this document states what must be true of it.

The model is deliberately bounded to **guards, token types, claim bindings,
share flags, and transitions**. Everything else (routing, storage, image
processing) is out of scope and stays in code review.

## Vocabulary

### Principals

| Principal   | Credential                                                         | Authority                                      |
| ----------- | ------------------------------------------------------------------ | ---------------------------------------------- |
| `anonymous` | none                                                               | public page and asset routes only              |
| `admin`     | the identity token in the `jwt` cookie, or no-password mode        | every operation                                |
| `share`     | capability `(albumId, shareId[, password])` resolving to one album | operations allowed by the album's share policy |

A share resolves to `Share { showMetadata, showDownload, showUpload, exp }`
stored on its album. There is no server-side session: authority is either
carried in a signed token or re-derived from the DB on each request.

### Token types

Each token carries a `typ` claim; a token is accepted only by a guard whose
expected type equals it.

| Type     | `typ`      | TTL  | Claims carried                                  | Minted by                           | Decoded by                                 |
| -------- | ---------- | ---- | ----------------------------------------------- | ----------------------------------- | ------------------------------------------ |
| identity | `admin`    | 14 d | `role` (admin or resolved share)                | login, server-side share resolution | `GuardAuth`, `GuardShare`, `GuardUpload`   |
| snapshot | `snapshot` | 300s | `resolvedShareOpt`, `timestamp`                 | prefetch, timestamp renewal         | `GuardTimestamp`, `TimestampGuardModified` |
| asset    | `asset`    | 300s | `allowOriginal`, `hash`, `assetId`, `timestamp` | get-data rows, hash renewal         | `GuardHash`, `GuardHashOriginal`           |

### Guards

| Guard               | Admits                                                                    | Denial |
| ------------------- | ------------------------------------------------------------------------- | ------ |
| `GuardAuth`         | `admin`                                                                   | 401    |
| `GuardShare`        | `share` (headers/query/password) or `admin` (cookie fallback)             | 401    |
| `GuardTimestamp`    | a `snapshot` token whose `timestamp` equals the query                     | 401    |
| `GuardHash`         | an `asset` token whose `hash` equals the URL path segment                 | 401    |
| `GuardHashOriginal` | an `asset` token with `allowOriginal` and matching `assetId`              | 401    |
| `GuardUpload`       | a `share` with `showUpload` and a matching `presignedAlbumId`, or `admin` | 401    |
| `GuardReadOnlyMode` | everyone while read-only mode is off                                      | 405    |

### Route classes

A route's class is a predicate over its guard set and its effects; the checker
derives it from the route attribute and handler signature rather than a stored
per-route list.

| Class           | Predicate                                              |
| --------------- | ------------------------------------------------------ |
| `public`        | no authentication guard                                |
| `admin-only`    | carries `GuardAuth` and no share-accepting guard       |
| `share-capable` | carries a guard that admits `share`                    |
| `serving`       | delivers asset bytes (`GuardHash`/`GuardHashOriginal`) |
| `mutating`      | carries `GuardReadOnlyMode`                            |
| `transition`    | mints or renews a token (login, prefetch, renewals)    |

## Properties

Each property is an invariant. "Enforced by" is the mechanism that makes it
true; "Derived check" is how a violation is caught — a static rule, a test, or
"by construction" where the type system already excludes it.

### AUTH-1 — Token type agreement

**Statement.** A token presented to a guard is accepted only when its `typ`
claim equals the type that guard expects. The three types are mutually
non-decodable.
**Scope.** Every JWT decode site.
**Enforced by.** `decode_typed` plus `deny_unknown_fields` on the claims
structs (`backend/src/router/auth.rs`).
**Derived check.** Static decode-agreement rule (each guard decodes the type the
model assigns it); token × guard cross-product negative tests.

### AUTH-2 — Guard rejection is propagated

**Statement.** A handler never runs on the strength of a guard it ignored. Every
`GuardResult<T>` binding's rejection is propagated (or explicitly handled).
**Scope.** Every handler argument of a `GuardResult` type.
**Enforced by.** The `?` operator in handler bodies (convention, not the type
system, because `GuardResult` catches the guard's error).
**Derived check.** Static rejection-propagation rule: a `GuardResult` binding
never referenced, or discarded without `?`, is a finding.

### AUTH-3 — Guard denial is an error, not a fall-through

**Statement.** A guard that denies a request returns `Outcome::Error` with the
declared status. `Outcome::Forward` is reserved for selecting the next ranked
route and is never used to deny.
**Scope.** Every `impl FromRequest`.
**Enforced by.** Guard implementations.
**Derived check.** Static rule: `Outcome::Forward` in a guard is a finding
unless the guard is a recorded route-variant exception (none today).

### AUTH-4 — Guard witness is consumed

**Statement.** When a route's target is an external resource (a body
`albumId`/`assetId`, a snapshot index, or a path asset id), a guard that carries
a resolved share must be the authority for that target: the target is derived
from, or checked against, the guard's claims.
**Scope.** Handlers carrying `GuardShare` or `GuardTimestamp` that name an
external resource.
**Enforced by.** Handler bodies reading `guard.claims`.
**Derived check.** Static witness-consumption rule; principal × scope tests
asserting absence of effect, not just status.

### AUTH-5 — Share policy is honoured

**Statement.** A response produced through a share never contains data the share
hides: `showMetadata=false` ⇒ no metadata field (including the stored path);
`showDownload=false` ⇒ no original bytes and no `allowOriginal` token.
**Scope.** Every share-capable read path.
**Enforced by.** `resolve_show_download_and_metadata` and the clearing helpers.
**Derived check.** Flag-preservation tests.

### AUTH-6 — Share is bound to its album

**Statement.** A share may read or write only assets in its own album. Its
snapshot is scoped to that album, and a write target outside it is refused.
**Scope.** Every share-capable route.
**Enforced by.** The prefetch album expression and the claim-binding checks in
the write handlers.
**Derived check.** Principal × scope tests (share A vs share B vs admin) with
"target unchanged after a refused write".

### AUTH-7 — Snapshot is bound

**Statement.** A snapshot token addresses exactly one snapshot: the `timestamp`
claim equals the query `timestamp`. A token for one snapshot cannot read
another.
**Scope.** Every snapshot-guarded route.
**Enforced by.** `GuardTimestamp`.
**Derived check.** Snapshot-mismatch tests.

### AUTH-8 — Asset is bound

**Statement.** An asset token authorizes exactly the one asset its claims name.
**Scope.** `GET /object/compressed/...` and `GET /object/imported/...`.
**Enforced by.** `GuardHash`, `GuardHashOriginal`.
**Derived check.** Cross-asset serving tests.

### AUTH-9 — Share state is live

**Statement.** A share's existence, expiry, and password are re-read and
re-validated on every request that uses it, and again at renewal before
re-issue. Disabling or expiring a share takes effect immediately, for identity
and for renewal.
**Scope.** `GuardShare`, the renewal handlers.
**Enforced by.** `resolve_share_internal` (per request) and
`revalidate_share_record` (renewal).
**Derived check.** Disabled/expired-share tests, including renewal.

### AUTH-10 — Renewal is bound to the presenter

**Statement.** A renewal extends only the presenter's own capability: the
presenter's share equals the submitted token's embedded share (admin excepted),
and the embedded share is re-validated from the DB before re-issue.
**Scope.** `POST /post/renew-timestamp-token`, `POST /post/renew-hash-token`.
**Enforced by.** The renewal handlers.
**Derived check.** Transition tests (presenter ≠ token share; expired share).

### AUTH-11 — Admin operations are admin-only

**Statement.** Operations that manage the server (config, shares, index,
rebuild, delete, tags/flags/rating, metadata edits by admin) require `admin`;
share credentials are not accepted for them.
**Scope.** Every `admin-only` route.
**Enforced by.** `GuardAuth`.
**Derived check.** Route–policy parity: an admin-only route must carry
`GuardAuth` and no share-accepting guard.

### AUTH-12 — Read-only mode blocks mutation

**Statement.** Every mutating route carries `GuardReadOnlyMode` and answers 405
while the mode is on; no mutation is reachable without it.
**Scope.** Every `mutating` route.
**Enforced by.** `GuardReadOnlyMode`.
**Derived check.** Static guard-completeness rule over mutating routes.

### AUTH-13 — Credentials are not over-shared

**Statement.** Secrets (`password`, `authKey`) are not exposed to a share, and
bearer tokens are not accepted from the URL query string.
**Scope.** Config read/export, token extraction.
**Enforced by.** `GuardAuth` on export; header-only bearer extraction.
**Derived check.** Secret-handling rules.

### AUTH-14 — Identity is stateless

**Statement.** No server-side session store decides authority; every request is
authorized from its token and the DB alone, and the server is restart-safe with
respect to authentication.
**Scope.** The whole subsystem.
**Enforced by.** Architecture (no session table, no in-memory session).
**Derived check.** Review (no executable check; recorded so it is a conscious
constraint).

## Deriving rules and tests

Each property names its derived check above. The mapping is the traceability
matrix the gate enforces:

- **AUTH-1, AUTH-3** → static rules over guard `FromRequest` impls and decode
  sites (`utils/openapi-sanity`).
- **AUTH-2, AUTH-4, AUTH-11, AUTH-12** → static rules over handler signatures and
  bodies.
- **AUTH-5…AUTH-10** → negative tests: principal × scope, token × guard,
  transition pre/post-conditions.
- **AUTH-13** → static rules.
- **AUTH-14** → review.

A property is not "done" until its derived check exists and runs; a new route,
guard, or token type must be classified against every property before it can
merge. The executable gate that holds this mapping is `.plan/authz-check.md`;
this document is the model it reads from.

## Encoding

The vocabulary above (principals, token types, guards, route classes) and the
property ids are the machine-readable surface. The intended encoding is a single
data file under `backend/` (a `tags.json`-style manifest) that both the static
gate and the tests read, so the model has one authority and the code is checked
against it in both directions. The encoding is the first implementation step in
`.plan/authz-check.md`.
