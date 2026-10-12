# auth2_guard_propagation

Dylint lint `AUTH2_GUARD_PROPAGATION` (Warn): every `GuardResult<T>`
request-guard binding must be propagated with `?` or otherwise handled.

## What it catches

A handler parameter counts as a guard binding iff its declared type resolves
(HIR) to the `router::GuardResult` type alias **and** the body's typeck
results say the binding's type is `core::result::Result` (diagnostic item).
A same-spelled lookalike alias, or an alias over a non-`Result` type, cannot
match.

Uses are classified by parent node: `let _ = x;` / `drop(x)` = discard;
`let b = x;` (binding pat, direct init) = neutral — this covers the
compiler-generated `let param = param;` that moves `async` fn params into
the future, plus user aliasing, followed transitively as further names for
the value; anything else (`?`, `match`, field access, call args) =
propagation. Fail closed: flag unless at least one use propagates
(zero uses also flags — an unenforced guard is a violation).

`?` needs no special case: it desugars to `match branch(x) { ... }`, so the
binding use sits under a call, never directly under a `let`.

## Why it matters

AUTH-2 (guard propagation): a `GuardResult` parameter is the endpoint's
proof that its auth guard ran. If the handler discards the binding, the
guard is never enforced and the endpoint executes for unauthenticated
callers — the declared 401 becomes a 200 with data. The lint fails closed
(zero uses flag) because an unenforced guard is silent at runtime: no
error, no log, just missing denial.

No live violations exist on the tree (zero findings); the lint is a
ratchet against regressions of the AUTH-2 discipline.

## Example

Violating code (warns on the `auth` binding):

```rust
pub async fn get_tags(auth: GuardResult<GuardAuth>) -> AppResult<Json<Vec<TagInfo>>> {
    let _ = auth; // warning: a `GuardResult` guard binding is discarded without propagation
    // ...
}
```

Fixed code (silent — `?` enforces the guard, `_` drops only the success value):

```rust
pub async fn get_tags(auth: GuardResult<GuardAuth>) -> AppResult<Json<Vec<TagInfo>>> {
    let _ = auth?;
    // ...
}
```

## Known limits

- A parameter spelled directly as `Result<T, AppError>` (bypassing the
  alias) is not tracked — the discipline mandates the `GuardResult` spelling.
- `if let Ok(..) = guard` without a diverging else is not yet detected
  (match-scrutinee analysis is open work).
- `#[allow(auth2_guard_propagation)]` / `#[deny(...)]` work as with any lint.

## Layout

Standalone Dylint library crate: `src/lib.rs` holds the whole lint.
It is deliberately _not_ a member of the main workspace and builds under
the pinned nightly from the parent `utils/dylint-lints/rust-toolchain.toml`.
Run via `just dylint-check` (see `utils/dylint-lints/README.md`).
