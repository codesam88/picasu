# decode_agreement

Dylint lint `DECODE_AGREEMENT` (Warn): each `FromRequest` guard must decode
exactly the claims type the model assigns it (AUTH-1/F1 ratchet).

## What it catches

The enclosing impl counts iff its trait ref resolves (HIR `Res::Def`) to a
`FromRequest` trait under a `request` module (same check as
`auth3_no_forward_denial`), and the guard identity is the impl's `Self` type
name. Inside such an impl, a call counts iff its callee resolves to the
`decode_typed` free function.

The decode target `T` is read from the call's typeck type (`Result<T, E>` —
first generic argument of the `Result` ADT), so turbofish
(`decode_typed::<ClaimsAsset>(..)`) and inferred
(`let claims: ClaimsAsset = decode_typed(..)`) spellings resolve identically.
Fail closed: an unresolvable target warns, and a direct `decode_typed` call
inside a guard with no assigned target warns.

## Why it matters

AUTH-1 (type soundness) / F1 (type-confusion class): a guard that decodes
the wrong claims type accepts tokens minted for another purpose — e.g. a
snapshot token satisfying an asset guard. The runtime `typ` check inside
`decode_typed` is the backstop; this lint is the static ratchet that the
guard → claims assignment never drifts.

Expected table (established empirically from `backend/src/router/auth.rs` —
every `decode_typed` call site per guard impl, verified before pinning):

| Guard impl          | Decode target                                                                                                                    | Site                              |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------- | --------------------------------- |
| `GuardAdmin`        | `ClaimsSession` (via `try_jwt_cookie_auth` → `decode_typed::<ClaimsSession>`) — no direct call in the impl                       | `try_jwt_cookie_auth`             |
| `GuardUser`         | `ClaimsSession` (inferred annotation)                                                                                            | `GuardUser::from_request`         |
| `GuardHash`         | `ClaimsAsset` (inferred annotation)                                                                                              | `GuardHash::from_request`         |
| `GuardHashOriginal` | `ClaimsAsset` (inferred annotation)                                                                                              | `GuardHashOriginal::from_request` |
| `GuardRenewHash`    | `ClaimsSnapshot` (turbofish)                                                                                                     | `GuardRenewHash::from_request`    |
| `GuardSnapshot`     | `ClaimsSnapshot` (inferred annotation)                                                                                           | `GuardSnapshot::from_request`     |
| `GuardPresenter`    | none — resolves (share-table lookup minting `ClaimsSession::new_share`, falling back to `try_jwt_cookie_auth` → `ClaimsSession`) | —                                 |
| `GuardMutate`       | none — config flag, no token                                                                                                     | —                                 |

Renewal handlers (not guard impls, out of the lint's scope) agree as well:
`renew_hash_token` decodes `ClaimsAsset` and `renew_timestamp_token` decodes
`ClaimsSnapshot`, both with `VALIDATION_ALLOW_EXPIRED`. No disagreement was
found — the lint is GREEN on day one.

## Example

Violating code (warns on the `decode_typed` call):

```rust
impl<'r> FromRequest<'r> for GuardHash {
    type Error = GuardError;
    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let token = extract_bearer_token(req)?;
        // warning: `GuardHash` decodes `ClaimsSession` but the model assigns `ClaimsAsset`
        let claims: ClaimsSession = match decode_typed(token, &VALIDATION) {
            Ok(claims) => claims,
            Err(err) => return Outcome::Error((Status::Unauthorized, err.into())),
        };
        // ...
    }
}
```

Fixed code (silent):

```rust
        let claims: ClaimsAsset = match decode_typed(token, &VALIDATION) {
            Ok(claims) => claims,
            Err(err) => return Outcome::Error((Status::Unauthorized, err.into())),
        };
```

## Known limits

- Only direct `decode_typed` calls inside the `FromRequest` impl are
  checked; the transitive decode inside `try_jwt_cookie_auth` (used by
  `GuardAdmin`/`GuardPresenter`) is documented in the table, not
  followed.
- Handler-body decodes (the two `ALLOW_EXPIRED` renewal paths) are outside
  the lint's scope; they are covered by review against the table above.
- A `decode_typed` renamed on import still matches by resolution; only the
  resolved name `decode_typed` is recognized.
- `#[allow(decode_agreement)]` / `#[deny(...)]` work as with any lint.

## Layout

Standalone Dylint library crate: `src/lib.rs` holds the whole lint.
It is deliberately _not_ a member of the main workspace and builds under
the pinned nightly from the parent `utils/dylint-lints/rust-toolchain.toml`.
Run via `just dylint-check` (see `utils/dylint-lints/README.md`). UI fixtures in `ui/` pin behavior (`cargo test` under the pinned nightly runs them via `dylint_testing`).
