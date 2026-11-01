# Authentication Model

This is the formal model of Picasu's authentication and authorization. It is the
source from which the enforcement rules and the security tests are derived:
`docs/auth.md` describes the mechanism, this document defines what must be true
of it.

The model is bounded to **authority, guards, routes, token derivation, and share
state**. Routing, storage, and image processing are out of scope.

## 1. Trust assumptions

- **Unforgeable tokens.** A token cannot be minted without `auth_key`; validation
  pins HS256. Tokens are therefore treated as authentic capability assertions.
- **Trusted perimeter.** The deployer's network or reverse proxy is the trust
  boundary (self-hosted threat model).
- **DB is the only mutable state.** There is no session store; every request's
  authority comes from its token and the DB.
- **A guard is a total decision.** It either establishes a capability or rejects;
  there is no "partial" acceptance.

## 2. Domains and notation

| Symbol        | Meaning                                                                                                                        |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `A`           | albums, identified by album id                                                                                                 |
| `X`           | assets, identified by asset id                                                                                                 |
| `T`           | snapshot ids (millisecond epoch integers)                                                                                      |
| `album(x)`    | the album an asset `x` belongs to                                                                                              |
| `scope(t)`    | the album a snapshot `t` is scoped to (⊥ = unscoped/admin)                                                                     |
| `F ⊆ {M,D,U}` | a share's flags: `M` metadata, `D` download, `U` upload (recorded but not enforced — shares are read-only regardless of flags) |
| `sh`          | a share record `⟨a, F, e, w⟩`: album, flags, expiry `e`, password `w`                                                          |

A share is **live** iff it exists in the DB and `e = ⊥ ∨ now ≤ e` and (if `w ≠ ⊥`)
the presented password equals `w`.

## 3. Authority

Authority is a set of capabilities with a lattice order defined by what they
permit:

```
Act  ::= list(t) | read_meta(x) | read_thumb(x) | read_orig(x)
       | write_meta(x) | write_album(a) | upload(a) | read_config | manage
Res  ::= T ∪ X ∪ A
Cap  ::= ⊤ | Share(a,F) | Snap(a,F,t) | Asset(x,o) | ⊥
```

`holds : Cap → 2^(Act × Res)` is the permission set; the order is
`c ⊑ c' ⟺ holds(c) ⊆ holds(c')`, with `⊥` empty and `⊤` all of `Act × Res`.

| Cap           | `holds(c)` (conditions in brackets)                                                                                          |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `Share(a,F)`  | `list(t)` `[scope(t)=a]`, `read_meta(x)` `[x∈a ∧ M∈F]`, `read_thumb(x)` `[x∈a]`, `read_orig(x)` `[x∈a ∧ D∈F]`, `read_config` |
| `Snap(a,F,t)` | `list(t)`, `read_meta(x)` `[x∈a ∧ M∈F]`, `read_thumb(x)` `[x∈a]`, `read_orig(x)` `[x∈a ∧ D∈F]`                               |
| `Asset(x,o)`  | `read_thumb(x)`, `read_orig(x)` `[o]`                                                                                        |
| `⊤`           | `Act × Res`                                                                                                                  |
| `⊥`           | `∅`                                                                                                                          |

`bound : Cap → 2^Res` is the resource scope a capability is confined to:

```
bound(⊤)           = Res
bound(Share(a,F))  = {a} ∪ {x | album(x)=a}
bound(Snap(a,F,t)) = {t} ∪ {x | album(x)=a}
bound(Asset(x,o))  = {x}
bound(⊥)           = ∅
```

## 4. Guard denotation

A guard denotes a partial map from a request to the capability it establishes,
`⟦g⟧ : Req ⇀ Cap`. Undefined means the guard rejects.

| Guard               | `⟦g⟧(req)`                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `GuardAdmin`        | `⊤` if the request carries a user identity with the admin role (cookie)                                                                                                                                                                                                                                                                                                                                                        |
| `GuardUser`         | the request's `User` identity (admin or not) if its record is live; rejects share tokens                                                                                                                                                                                                                                                                                                                                       |
| `GuardPresenter`    | the request's `User` identity if its cookie identity record is live (`⊤` under the admin role, the viewer identity otherwise); else `Share(a,F)` if it resolves a **live** share `⟨a,F,e,w⟩`                                                                                                                                                                                                                                   |
| `GuardSnapshot`     | `Snap(a,F,t)` if it decodes a snapshot token `⟨caller,a,F,t⟩`, `t` equals the query `timestamp`, and the resolved presenter's identity equals `caller` (user id) or the embedded share (share pair); `caller` is empty only for share-minted tokens                                                                                                                                                                            |
| `GuardHash`         | `Asset(x,o)` if it decodes an asset token `⟨x,o,t,caller,sh⟩`, `hash(x)` equals the URL path segment, and the resolved presenter's identity equals `caller` (or the embedded share for share-minted tokens)                                                                                                                                                                                                                    |
| `GuardHashOriginal` | `Asset(x,o)` if it decodes `⟨x,o,t,caller,sh⟩` with `o = true`, `x` equals the URL path segment, and the resolved presenter's identity equals `caller` (or the embedded share)                                                                                                                                                                                                                                                 |
| `GuardMutate`       | `⊤` if read-only mode is off                                                                                                                                                                                                                                                                                                                                                                                                   |
| `GuardReauth`       | `⊤`-with-proof if the request carries a live session identity of any role and the `X-Picasu-Password` header verifies (constant-time) against that caller's stored argon2id hash; `⊥` otherwise (401 for no/invalid session, 403 for missing/wrong password). Authority is not granted here — it comes from the route's other guards (lift = `GuardAdmin` ∩ re-auth, password change = `GuardUser` ∩ `GuardMutate` ∩ re-auth). |

A route `r` has a guard set `G(r)`. Every guard must accept:

```
accepts(r,req)  ⟺  ∀ g ∈ G(r): ⟦g⟧(req) ≠ ⊥
A(r,req)        =  ∪ { holds(⟦g⟧(req)) | g ∈ G(r) }      -- permissions
B(r,req)        =  ∩ { bound(⟦g⟧(req)) | g ∈ G(r) }      -- resource scope
```

`A` is the union because any accepting guard may authorize the action; `B` is
the intersection because the target must lie in every guard's scope. For
`GET /object/imported/<x>`, `B = bound(GuardPresenter) ∩ bound(GuardHashOriginal)`,
which requires `x` to be in the share's album _and_ equal the token's asset.

A guard denoted `⊤`-with-proof (such as `GuardReauth`) is a proof-carrying
gate, not an authority source: it contributes `⊤` to `A` and `Res` to `B`,
so it neither widens the permission set beyond what the route's other guards
establish nor narrows the resource scope; its only effect is on `accepts`,
which requires the proof to pass. No route carries it alone.

## 5. Route conformance

Each route declares what it does and on what resource:

- `needs(r) ⊆ Act × Res` — the permissions the handler exercises.
- `target(r, req) ∈ Res` — the resource the handler operates on, which for a
  share-guarded route must be **derived from the guard's claims**, not from
  untrusted input.

**Conformance (C1).** A route conforms iff for every request:

```
accepts(r,req)
  ⟹  needs(r) ⊆ A(r,req)
  ∧  (act(r), target(r,req)) ∈ A(r,req)
  ∧  target(r,req) ∈ B(r,req)
```

C1 is the central condition. Its three clauses are, respectively: the guards are
strong enough (AUTH-11, AUTH-12), the specific action on the target is permitted
(AUTH-5), and the target lies in every guard's scope (AUTH-4, AUTH-6, AUTH-7,
AUTH-8). When `accepts` is false the request must be denied (AUTH-2, AUTH-3).

## 6. Token derivation

Tokens are minted only along the delegation chain; each derivation attenuates.

```
prefetch:   Share(a,F)          ⊢  Snap(c,a,F,t)        (scope(t)=a; c = presenter id, or "" for a share)
get-data:   Snap(c,a,F,t)       ⊢  Asset(x, o := D∈F, c, sh)   (x∈a)
renew(t):   cap(t)              ⊢  t'                   (payload(t')=payload(t); caller equality per AUTH-10)
```

**Attenuation (A1).** For every derivation `c ⊢ c'`:
`holds(c') ⊆ holds(c)` and `bound(c') ⊆ bound(c)`.

For the chain this yields
`holds(Asset(x,o)) ⊆ holds(Snap(a,F,t)) ⊆ holds(Share(a,F))`, so an asset token
can never read an original the share hides and never leaves its album.

## 6.5 Personal areas (ownership)

Personal areas are not a new capability in the lattice above: a session
identity still denotes `⊤` for its holder. Ownership is a path-level
predicate applied in addition to capability checks, derived from the
filesystem on every request.

**Principals.** For data access an admin is a regular user: no
cross-user album, photo, or share visibility. The relevant principals are
`owner(u)` (the user whose personal subtree a path lies under), `non-owner`
(any authenticated user with `u ≠ owner`), and `admin-as-user` (an admin
acting on data, identical to a non-admin user of the same id). Anonymous
share holders and snapshot bearers have no personal area and no ownership
(`caller = ""`).

**Ownership derivation.** Configured roots: `image_home` (public) and an
optional `users_home` (personal). `classify_path(canonical)` returns
`Public`, `Personal { owner }`, `UsersRoot`, or `Outside`. `owner` is the
first path segment directly under `users_home` after canonicalization;
the path is otherwise uninterpreted. The filesystem is the source of truth:
no stored owner column exists, and moving or renaming directories changes
ownership accordingly, so a rebuild from disk preserves it. When
`users_home` is unset, every path classifies as `Public` or `Outside` and
all gates pass through (feature disabled). `users_home` must not equal or
nest `image_home` in either direction (rejected at boot). The root itself
is never an album, addressable, movable, shareable, or scannable.

**Per-operation scoping.**

- _Reads_ (`get-albums`, prefetch/get-data, fs completion, tags/counts):
  public paths are visible to every caller; a personal path is visible
  only to its owner; `UsersRoot` is never exposed. A non-owner does not
  receive a 403 — the album is absent from `get-albums` and yields no
  rows.
- _Timeline opt-in:_ personal rows (media and albums) appear in the
  prefetch/timeline only when `include_personal = true` **and** the caller
  owns them; other users' rows are already excluded by visibility. The
  flag defaults to `false` and is enforced server-side regardless of any
  client-supplied filter or `basicString`; the cache key includes the
  caller and the flag.
- _Writes_ (create/move/assign/delete/upload/flags): `write_gate(caller,
source, dest)` refuses `UsersRoot`, foreign personal paths, and
  cross-owner pairs, with no partial effect (refusal happens before the
  first rename). Admins receive no exception.
- _Shares:_ create/edit/delete requires `visible_to(caller, album path)`;
  a personal album is shareable only by its owner. Share listings are
  filtered to `created_by == caller`. Resolving a share on a personal
  album works for anonymous holders exactly as for public albums (the
  share is album-scoped).
- _Scan:_ admin Scan Now walks the global root plus all personal subtrees
  per the `root` selector (`"global"`, `"personal"`, `"auto"`); the
  `users_home` root itself never gains an album record and is pruned if
  one exists. An unknown selector is a 400; requesting `"personal"` while
  disabled is a 400.

**404-vs-403 (no oracle).** Management of a foreign personal album's
shares and any operation naming an invisible personal album returns 404
("Album not found"), not 403, so existence is not leaked. Listing and
read paths simply omit the row.

**Home provisioning.** `ensure_personal_home(id)` creates
`<users_home>/<id>` and its dir-album record when the feature is enabled;
it is a best-effort no-op when disabled or when the id is invalid. It runs on the two user-creation paths (`create_user` and
`provision_user_with_home`); admin role changes do not re-provision.
Users created before `users_home` was enabled (or via the offline CLI) are
covered by the next admin Scan Now, which also indexes empty directories.

## 7. Invariants

Each is a named formula over the model. `AUTH-n` are the identifiers the rules
and tests cite.

**AUTH-1 — Type soundness.** `decode_g(tok) ≠ ⊥ ⟹ typ(tok) = expected(g)`, where
`expected` maps `GuardAdmin` → `session` (identity token, admin role required),
`GuardPresenter`/`GuardUser` → identity token of any role,
`GuardSnapshot` → `snapshot`, `GuardHash`/`GuardHashOriginal` → `asset`.
Decoding a token as a type it does not declare fails.

**AUTH-2 — Rejection is propagated.** For every route, `¬accepts(r,req)` implies
the request is denied. (The `GuardResult` wrapper catches a guard error, so the
implementation must re-establish this by propagating with `?`.)

**AUTH-3 — Denial is an error, not a fall-through.** A guard that rejects
establishes no capability (`⟦g⟧` undefined) and returns an `Outcome::Error` with
the declared status. `Outcome::Forward` is used only to select the next ranked
route, never to deny — a denial must not be re-enterable as a different route
with a weaker `needs`.

**AUTH-4 — Witness binding.** In C1's third clause, `target(r,req)` is computed
from `⟦G(r)⟧(req)`'s claims; equivalently, `target(r,req) ∈ bound(cap)` for the
established `cap`. A target read from the body or query is a violation.

**AUTH-5 — Flag soundness.** `(read_meta(x), x) ∈ needs(r)` via a share cap
requires `M ∈ F`; `(read_orig(x), x)` requires `D ∈ F`. Uploads require a
user session (`GuardUser`) — the share `U` flag is recorded but confers
nothing. Follows from §3; the implementation must realize it.

**AUTH-6 — Album scoping.** For any `(act, x)` with `act ∈ {read_*}` established
by a share cap, `album(x) = a`. Share caps establish no writes, so there is
no share-established `write_album` to scope.

**AUTH-7 — Snapshot binding.** `⟦GuardSnapshot⟧(req) = Snap(a,F,t) ⟹ t =`
`query_timestamp(req)`; a token for one snapshot cannot read another.

**AUTH-8 — Asset binding.** `⟦GuardHash⟧`/`⟦GuardHashOriginal⟧` accept only when
the URL path resource equals the token's `hash`/`assetId`; an asset token
authorizes exactly one asset.

**AUTH-9 — Live identity, immutable capabilities.** Identity resolution re-reads
the DB (`GuardPresenter` requires a live share). Minted capabilities are immutable
and remain valid until `exp`; revoking a share prevents new identity and renewal
but does not retract outstanding tokens. A renewal re-validates the embedded
share before re-issue. For users the role is re-read per request: demotion or
removal takes effect before expiry, while promotion requires re-login (the
token-embedded flag gates first).

**AUTH-10 — Renewal preserves authority.** `renew(t) = t'` requires
`payload(t') = payload(t)`, `bound(t') = bound(t)`, and a presenter capability
`c ⊒ cap(t)` whose identity equals the token's caller (a user presenter
renews only snapshots whose `caller` is their user id; a share presenter
renews only its own share's snapshot; admins are not exempt); the embedded
share is live. Renewal can neither widen scope nor outlive the share's
validity for new tokens.

**AUTH-11 — Admin isolation.** If `manage ∈ needs(r)`, then `G(r)` establishes
`⊤` and admits no share capability: the caller must be a user with the admin
role (`GuardAdmin`), not merely a share holder.

**AUTH-12 — Read-only blocks mutation.** If `r` has an effect (any `write_*`,
`upload`, `manage`), then `GuardMutate ∈ G(r)`, except
`POST /post/admin/config/disable-read-only`, which lifts the kill-switch and
is gated by `GuardAdmin` ∩ `GuardReauth` (the admin's password proof).

**AUTH-13 — Credential isolation.** `(manage, config-secrets) ∉ holds(Share(a,F))`
for every share; bearer tokens are extracted from the `Authorization` header
only.

**AUTH-14 — Statelessness.** The authority of a request is a function of its
token and the DB alone; no component's decision depends on a server-side session.

**AUTH-15 — Privileged re-authentication.** For any route with
`GuardReauth ∈ G(r)`, `accepts(r, req)` requires the password proof in
`X-Picasu-Password` verified against the caller's stored hash; a valid
session token alone is insufficient. Applies to
`POST /post/admin/config/disable-read-only` (lift) and
`PUT /put/users/password` (self-change and admin reset). Pinned by the
re-auth cases in `backend/src/router/put/users_password.rs` and
`backend/src/router/post/admin_config.rs`.

**AUTH-16 — Zero-admin invariant.** Deletion and demotion cannot leave zero
admins: `user_store::delete` and `set_admin_role` share the `Conflict`
path, and the check and the write are one atomic transaction (single redb
writer), so concurrent operations cannot strand zero admins. Pinned by
the delete matrix in `backend/src/router/delete.rs` and the demotion cases
in `backend/src/router/put/users_admin.rs`.

**AUTH-17 — Personal read isolation.** For any read on a path, album, or
asset, if `classify_path(path) = Personal { owner }` then the caller must
equal `owner`; a path classified `UsersRoot` is never returned, read, or
counted. A non-owner observes absence (omitted row / empty result), never
a 403, so existence is not leaked. Pinned by the `get-albums` filtering
in `backend/src/router/get/get_list.rs` and `row_visible` in
`backend/src/router/get/get_prefetch.rs`.

**AUTH-18 — Personal write isolation.** For any mutation with source
`s` and/or destination `d`, `write_gate(caller, s, d)` must accept;
otherwise the request is refused before any filesystem or DB effect, and
the affected target is left unchanged. Refused cases: either path is
`UsersRoot`; either personal path's owner differs from `caller`; or both
are personal with differing owners. Admins receive no exception. Pinned
by `write_gate` in `backend/src/storage/files.rs` and the per-op tests.

**AUTH-19 — Share creator-ownership.** A share may be created, edited, or
deleted on an album only when `visible_to(caller, album path)` holds; for
a personal album this means `caller` is its owner. Share listings expose
only entries with `created_by == caller`. Management of an invisible
album returns 404, not 403. Pinned by `create_share`, `edit_share`, and
`delete_share` plus `share_visible_to`.

**AUTH-20 — Timeline personal opt-in.** Personal rows (media and albums)
appear in prefetch/timeline results only when `include_personal = true`
and the caller owns them. The flag defaults to `false` and is enforced
server-side independent of any client filter; the cache key includes the
caller and the flag. Pinned by `row_visible` in
`backend/src/router/get/get_prefetch.rs`.

## 8. From the model to executable checks

The model is not executable as written; each invariant is discharged by one of
three means. The static rules are decision procedures for a syntactic fragment;
the tests are witnesses.

| Invariant | Means       | Derived artifact                                                                                                                                            |
| --------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| AUTH-1    | static      | decode-agreement: each guard's decode target equals `expected(g)`                                                                                           |
| AUTH-2    | static      | rejection-propagation: every `GuardResult` binding consumed (`?`)                                                                                           |
| AUTH-3    | static      | no `Outcome::Forward` in a guard impl (route-variant exceptions recorded)                                                                                   |
| AUTH-4    | static+test | witness-consumption rule; principal × scope tests asserting absence of effect                                                                               |
| AUTH-5    | test        | flag-preservation tests                                                                                                                                     |
| AUTH-6    | static+test | C1 scope clause; principal × scope tests                                                                                                                    |
| AUTH-7/8  | test        | snapshot- and asset-mismatch tests                                                                                                                          |
| AUTH-9    | test        | disabled/expired-share tests, including renewal                                                                                                             |
| AUTH-10   | static+test | renewal binding rule; transition tests                                                                                                                      |
| AUTH-11   | static      | route–policy parity: `manage` routes carry `GuardAdmin` and no share guard                                                                                  |
| AUTH-12   | static      | guard-completeness: every mutating route carries `GuardMutate`, except the read-only lift (`disable-read-only`), which carries `GuardAdmin` ∩ `GuardReauth` |
| AUTH-13   | static      | secret-handling rules                                                                                                                                       |
| AUTH-14   | review      | recorded constraint; no executable check                                                                                                                    |
| AUTH-15   | static+test | privileged-route guard sets include `GuardReauth`; re-auth pins in `users_password.rs` and `admin_config.rs`                                                |
| AUTH-16   | test        | zero-admin refusal on delete and demote (Conflict), including concurrency                                                                                   |
| AUTH-17   | static+test | personal read isolation: `get-albums`/prefetch omit foreign personal rows and `UsersRoot`; `visible_to` gate                                                |
| AUTH-18   | static+test | personal write isolation: `write_gate` refuses `UsersRoot`/foreign/cross-owner; target unchanged on refusal                                                 |
| AUTH-19   | static+test | share creator-ownership: `visible_to` on create/edit/delete; `share_visible_to` on listing; 404 on invisible                                                |
| AUTH-20   | test        | timeline personal opt-in: `row_visible` gated on `include_personal`; cache key includes caller + flag                                                       |

A new route, guard, or token type is classified against every invariant before
it can merge. A property is not "done" until its derived artifact exists and
runs.

## Appendix A — Personal-area route/invariant disposition

`docs/authz-coverage.md` is not present in this tree; the full route ×
invariant matrix is owned by `.plan/authz-check.md` (§4) and
`.plan/authz-props.md` (S0e), which are read-only here. This appendix
records only the personal-area additions so those plans can absorb them
without contradiction, and does not duplicate their matrices.

New query flag: `include_personal` on `POST /get/prefetch`
(`backend/src/router/get/get_prefetch.rs`) — default `false`, cache key
includes caller + flag (AUTH-20).

Root selector: `root` on `POST /post/index/album` (`"global"`,
`"personal"`, `"auto"`; unknown value → 400) (AUTH-11, AUTH-18).

`AlbumInfo` markers: `root` (`"global"`/`"personal"`/`null`) and `owner`
(user id or `null`) on `GET /get/get-albums`
(`backend/src/router/get/get_list.rs`) (AUTH-17).

Guard on shares: relaxed from `GuardAdmin` to `GuardUser` + ownership via
`visible_to` (AUTH-11, AUTH-19).

Hardening dispositions (assessed, not implemented in S6):

- CLI user provisioning: accepted gap. `picasu-cmd user add` writes the
  user store directly and does not call `ensure_personal_home`; users
  created this way are covered by the next admin Scan Now (which indexes
  empty directories), and `users_home` is typically unset in
  CLI-only/offline flows. No change.
- `users_dir_for` disabled/invalid distinction: verified present (distinct
  `UsersDirError::Disabled` vs `InvalidUser`); not reworked.

## 9. Encoding

The domains, `Cap`, `holds`, the guard denotations, and the invariant ids are the
machine-readable surface. The intended encoding is one data file under
`backend/` (a `tags.json`-style manifest) that the static gate and the tests both
read, so the model has a single authority and the code is checked against it in
both directions. Encoding it is not yet implemented.
