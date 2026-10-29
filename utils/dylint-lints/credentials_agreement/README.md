# credentials_agreement

Dylint lint `TOKEN_TYP_AGREEMENT` (Warn): a handler's `x-picasu-auth`
`credentials` must agree with the claims types its guards decode (AUTH-1/S3).

## What it catches

A handler annotated with `#[utoipa::path(...)]` carrying an `x-picasu-auth`
extension declares a `credentials` list and a set of guards (resolved from the
handler's parameters, unwrapping `GuardResult<T>`). The lint derives the
expected typ set from the guards:

| Guard                             | Derived typ        |
| --------------------------------- | ------------------ |
| `GuardAdmin`, `GuardUser`         | `session`          |
| `GuardHash`, `GuardHashOriginal`  | `asset`            |
| `GuardRenewHash`, `GuardSnapshot` | `snapshot`         |
| `GuardPresenter`                  | `session`, `share` |
| `GuardMutate`, `GuardReauth`      | (none)             |

The two sets must be equal in both directions: every guard-derived typ must
appear in `credentials`, and every `credentials` entry must be backed by a guard.
No guards requires an empty `credentials` (public ops). Unknown guard types fail
closed (warn).

## Why it matters

AUTH-1 (type soundness) / S3: a handler that accepts a token type its guards
do not actually decode (or omits one they do) can let a token minted for
another purpose satisfy the endpoint. The `typ` claim is the runtime backstop;
this lint is the static ratchet keeping the OpenAPI declaration honest.

## Example

Violating code (warns on the handler):

```rust
#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser"],
        "credentials": ["asset"]
    })))
)]
pub async fn handler(auth: GuardResult<GuardUser>) {}
```

Fixed code (silent):

```rust
#[utoipa::path(
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser"],
        "credentials": ["session"]
    })))
)]
pub async fn handler(auth: GuardResult<GuardUser>) {}
```

## Known limits

- Only handlers carrying `#[utoipa::path]` with an `x-picasu-auth` extension
  are checked; public endpoints without that extension are out of scope.
- The attribute is recovered by text search over the source preceding the
  function (the proc macro consumes it), not from HIR: odd spacing in the
  attribute path or prose containing `fn` between attribute and function
  silently skips the handler instead of warning.
- Guard types are taken from the handler's parameter list (unwrapping
  `GuardResult<T>`); guards obtained indirectly are not detected.
- `credentials` and guard sets are compared as sets (order-insensitive); duplicates
  are ignored.
- Unknown guard types fail closed (warn) rather than being silently ignored.

## Layout

Standalone Dylint library crate: `src/lib.rs` holds the whole lint.
It is deliberately _not_ a member of the main workspace and builds under
the pinned nightly from the parent `utils/dylint-lints/rust-toolchain.toml`.
Run via `just dylint-check` (see `utils/dylint-lints/README.md`). UI fixtures in `ui/` pin behavior (`cargo test` under the pinned nightly runs them via `dylint_testing`).
