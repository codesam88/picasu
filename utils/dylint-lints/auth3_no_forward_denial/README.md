# auth3_no_forward_denial

Dylint lint `AUTH3_NO_FORWARD_DENIAL` (Warn): no `Outcome::Forward` in a
`FromRequest` guard impl (AUTH-3: denial is an error, not a fall-through).
A guard that rejects must return `Outcome::Error` with the declared status;
`Outcome::Forward` only selects the next ranked route, never denies.

## What it catches

The enclosing impl counts iff its trait ref resolves (HIR `Res::Def`) to
a `FromRequest` trait under a `request` module (currently
`rocket::request::from_request::FromRequest`). A same-named user trait or
an inherent impl cannot match.

Inside such an impl, an `Outcome::Forward` path matches iff its `qself`
resolves to the `Outcome` enum (`QPath::TypeRelative` — the shape
hand-written `Outcome::Forward(..)` takes) or it resolves via its `Ctor`
def to an `Outcome` variant — or, fail closed, its spelling ends in
`Outcome::Forward` even when resolution is unavailable.

Rocket-generated route shims also mention `Outcome::Forward`, but those
live outside any `FromRequest` impl and never match (verified: ~116
generated mentions across the tree, zero findings).

## Why it matters

AUTH-3 (denial is an error, not a fall-through): Rocket's `Result<T, E>`
guard impl propagates `Forward` (`Forward(status) => Forward(status)`), so
a guard that "denies" with `Outcome::Forward` does not deny at all — the
request falls through to lower-ranked routes and the client sees a wrong
status with a wrong body. Two proven incidents on this tree (both in
`backend/src/router/auth.rs`):

- `GuardHashOriginal::from_request`: a valid token without
  `allow_original` returned `Forward(Unauthorized)`. The denied request
  fell through to the rank-11 `spa_fallback` GET catch-all, which served
  the SPA shell (200) instead of the declared 401.
- `GuardRenewHash::from_request` (2 sites, one cause): a missing
  or undecodable bearer token returned `Forward(Unauthorized)`. The guard
  is consumed directly by `POST /post/renew-hash-token` (no ranked
  alternative route), so denial surfaced as 404 instead of the documented 401.

Triage of the 3 existing sites: each is a genuine denial-as-forward, none
is legitimate route-variant selection, so no allowlist exception is
recorded. The three sites were fixed (denial-as-forward became denial-as-error), so the lint is clean on the current tree. If a future site is judged
legitimate route-variant selection, record it with a per-site
`#[allow(auth3_no_forward_denial)]` plus the reason — no project-wide
suppression lives in the lint.

## Example

Violating code (warns on the `Outcome::Forward` expression):

```rust
impl<'r> FromRequest<'r> for GuardMutate {
    type Error = GuardError;
    async fn from_request(_req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        if read_only {
            // warning: `Outcome::Forward` in a `FromRequest` guard impl:
            // denial must be `Outcome::Error`, `Forward` only selects the next ranked route
            return Outcome::Forward(Status::MethodNotAllowed);
        }
        Outcome::Success(GuardMutate)
    }
}
```

Fixed code (silent):

```rust
        if read_only {
            return Outcome::Error((
                Status::MethodNotAllowed,
                AppError::new(ErrorKind::ReadOnlyMode, "Read-only mode is enabled"),
            ));
        }
```

## Known limits

- An `Outcome` imported under a different name still matches by
  resolution; only a fully unresolvable spelling falls back to the
  `Outcome::Forward` name check. A `Forward` constructed outside a
  `FromRequest` impl (e.g. Rocket-generated route shims) never matches.
- `#[allow(auth3_no_forward_denial)]` / `#[deny(...)]` work as with any
  lint; use per-site `allow` with a recorded reason for any future
  route-variant exception.

## Layout

Standalone Dylint library crate: `src/lib.rs` holds the whole lint.
It is deliberately _not_ a member of the main workspace and builds under
the pinned nightly from the parent `utils/dylint-lints/rust-toolchain.toml`.
Run via `just dylint-check` (see `utils/dylint-lints/README.md`). UI fixtures in `ui/` pin behavior (`cargo test` under the pinned nightly runs them via `dylint_testing`).
