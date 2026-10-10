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

| Symbol        | Meaning                                                               |
| ------------- | --------------------------------------------------------------------- |
| `A`           | albums, identified by album id                                        |
| `X`           | assets, identified by asset id                                        |
| `T`           | snapshot ids (millisecond epoch integers)                             |
| `album(x)`    | the album an asset `x` belongs to                                     |
| `scope(t)`    | the album a snapshot `t` is scoped to (⊥ = unscoped/admin)            |
| `F ⊆ {M,D,U}` | a share's flags: `M` metadata, `D` download, `U` upload               |
| `sh`          | a share record `⟨a, F, e, w⟩`: album, flags, expiry `e`, password `w` |

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

| Cap           | `holds(c)` (conditions in brackets)                                                                                                                                                          |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Share(a,F)`  | `list(t)` `[scope(t)=a]`, `read_meta(x)` `[x∈a ∧ M∈F]`, `read_thumb(x)` `[x∈a]`, `read_orig(x)` `[x∈a ∧ D∈F]`, `write_meta(x)` `[x∈a]`, `write_album(a)`, `upload(a)` `[U∈F]`, `read_config` |
| `Snap(a,F,t)` | `list(t)`, `read_meta(x)` `[x∈a ∧ M∈F]`, `read_thumb(x)` `[x∈a]`, `read_orig(x)` `[x∈a ∧ D∈F]`                                                                                               |
| `Asset(x,o)`  | `read_thumb(x)`, `read_orig(x)` `[o]`                                                                                                                                                        |
| `⊤`           | `Act × Res`                                                                                                                                                                                  |
| `⊥`           | `∅`                                                                                                                                                                                          |

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

| Guard               | `⟦g⟧(req)`                                                                                                   |
| ------------------- | ------------------------------------------------------------------------------------------------------------ |
| `GuardAuth`         | `⊤` if the request carries a user identity with the admin role (cookie or open first-run mode)               |
| `GuardUser`         | the request's `User` identity (admin or not) if its record is live; rejects share tokens                     |
| `GuardShare`        | `⊤` if admin-role user cookie; else `Share(a,F)` if it resolves a **live** share `⟨a,F,e,w⟩`                 |
| `GuardTimestamp`    | `Snap(a,F,t)` if it decodes a snapshot token `⟨a,F,t⟩` and `t` equals the query `timestamp`                  |
| `GuardHash`         | `Asset(x,o)` if it decodes an asset token `⟨x,o,t⟩` and `hash(x)` equals the URL path segment                |
| `GuardHashOriginal` | `Asset(x,o)` if it decodes `⟨x,o,t⟩` with `o = true` and `x` equals the URL path segment                     |
| `GuardUpload`       | `Share(a,{U})` if it resolves a live share with `U ∈ F` and a matching `presignedAlbumId`; else `⊤` if admin |
| `GuardReadOnlyMode` | `⊤` if read-only mode is off                                                                                 |

A route `r` has a guard set `G(r)`. Every guard must accept:

```
accepts(r,req)  ⟺  ∀ g ∈ G(r): ⟦g⟧(req) ≠ ⊥
A(r,req)        =  ∪ { holds(⟦g⟧(req)) | g ∈ G(r) }      -- permissions
B(r,req)        =  ∩ { bound(⟦g⟧(req)) | g ∈ G(r) }      -- resource scope
```

`A` is the union because any accepting guard may authorize the action; `B` is
the intersection because the target must lie in every guard's scope. For
`GET /object/imported/<x>`, `B = bound(GuardShare) ∩ bound(GuardHashOriginal)`,
which requires `x` to be in the share's album _and_ equal the token's asset.

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
prefetch:   Share(a,F)          ⊢  Snap(a,F,t)          (scope(t)=a)
get-data:   Snap(a,F,t)         ⊢  Asset(x, o := D∈F)   (x∈a)
renew(t):   cap(t)              ⊢  t'                   (payload(t')=payload(t))
```

**Attenuation (A1).** For every derivation `c ⊢ c'`:
`holds(c') ⊆ holds(c)` and `bound(c') ⊆ bound(c)`.

For the chain this yields
`holds(Asset(x,o)) ⊆ holds(Snap(a,F,t)) ⊆ holds(Share(a,F))`, so an asset token
can never read an original the share hides and never leaves its album.

## 7. Invariants

Each is a named formula over the model. `AUTH-n` are the identifiers the rules
and tests cite.

**AUTH-1 — Type soundness.** `decode_g(tok) ≠ ⊥ ⟹ typ(tok) = expected(g)`, where
`expected` maps `GuardAuth`/`GuardShare`/`GuardUpload`/`GuardUser` → `admin`,
`GuardTimestamp` → `snapshot`, `GuardHash`/`GuardHashOriginal` → `asset`.
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
requires `M ∈ F`; `(read_orig(x), x)` requires `D ∈ F`; `(upload(a), a)` requires
`U ∈ F`. Follows from §3; the implementation must realize it.

**AUTH-6 — Album scoping.** For any `(act, x)` with `act ∈ {read_*, write_meta}`
established by a share cap, `album(x) = a`. For `write_album(a')`, `a' = a`.

**AUTH-7 — Snapshot binding.** `⟦GuardTimestamp⟧(req) = Snap(a,F,t) ⟹ t =`
`query_timestamp(req)`; a token for one snapshot cannot read another.

**AUTH-8 — Asset binding.** `⟦GuardHash⟧`/`⟦GuardHashOriginal⟧` accept only when
the URL path resource equals the token's `hash`/`assetId`; an asset token
authorizes exactly one asset.

**AUTH-9 — Live identity, immutable capabilities.** Identity resolution re-reads
the DB (`GuardShare` requires a live share). Minted capabilities are immutable
and remain valid until `exp`; revoking a share prevents new identity and renewal
but does not retract outstanding tokens. A renewal re-validates the embedded
share before re-issue. For users the role is re-read per request: demotion or
removal takes effect before expiry, while promotion requires re-login (the
token-embedded flag gates first).

**AUTH-10 — Renewal preserves authority.** `renew(t) = t'` requires
`payload(t') = payload(t)`, `bound(t') = bound(t)`, and a presenter capability
`c ⊒ cap(t)`; the embedded share is live. Renewal can neither widen scope nor
outlive the share's validity for new tokens.

**AUTH-11 — Admin isolation.** If `manage ∈ needs(r)`, then `G(r)` establishes
`⊤` and admits no share capability: the caller must be a user with the admin
role (`GuardAuth`), not merely a share holder.

**AUTH-12 — Read-only blocks mutation.** If `r` has an effect (any `write_*`,
`upload`, `manage`), then `GuardReadOnlyMode ∈ G(r)`.

**AUTH-13 — Credential isolation.** `(manage, config-secrets) ∉ holds(Share(a,F))`
for every share; bearer tokens are extracted from the `Authorization` header
only.

**AUTH-14 — Statelessness.** The authority of a request is a function of its
token and the DB alone; no component's decision depends on a server-side session.

## 8. From the model to executable checks

The model is not executable as written; each invariant is discharged by one of
three means. The static rules are decision procedures for a syntactic fragment;
the tests are witnesses.

| Invariant | Means       | Derived artifact                                                              |
| --------- | ----------- | ----------------------------------------------------------------------------- |
| AUTH-1    | static      | decode-agreement: each guard's decode target equals `expected(g)`             |
| AUTH-2    | static      | rejection-propagation: every `GuardResult` binding consumed (`?`)             |
| AUTH-3    | static      | no `Outcome::Forward` in a guard impl (route-variant exceptions recorded)     |
| AUTH-4    | static+test | witness-consumption rule; principal × scope tests asserting absence of effect |
| AUTH-5    | test        | flag-preservation tests                                                       |
| AUTH-6    | static+test | C1 scope clause; principal × scope tests                                      |
| AUTH-7/8  | test        | snapshot- and asset-mismatch tests                                            |
| AUTH-9    | test        | disabled/expired-share tests, including renewal                               |
| AUTH-10   | static+test | renewal binding rule; transition tests                                        |
| AUTH-11   | static      | route–policy parity: `manage` routes carry `GuardAuth` and no share guard     |
| AUTH-12   | static      | guard-completeness: every mutating route carries `GuardReadOnlyMode`          |
| AUTH-13   | static      | secret-handling rules                                                         |
| AUTH-14   | review      | recorded constraint; no executable check                                      |

A new route, guard, or token type is classified against every invariant before
it can merge. A property is not "done" until its derived artifact exists and
runs.

## 9. Encoding

The domains, `Cap`, `holds`, the guard denotations, and the invariant ids are the
machine-readable surface. The intended encoding is one data file under
`backend/` (a `tags.json`-style manifest) that the static gate and the tests both
read, so the model has a single authority and the code is checked against it in
both directions. Encoding it is the first step of `.plan/authz-check.md`.
