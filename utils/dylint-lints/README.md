# dylint-lints

Project-specific Rust lints for the authorization guard discipline, enforced
with full compiler type information via Dylint. They cover what text-level
analysis cannot: resolved types, cross-item guard lookup, desugared control
flow. Rules that need none of that stay in `utils/openapi-sanity`; the
disposition per rule is recorded in `.plan/api-assurance.md`.

## Lints

### AUTH2_GUARD_PROPAGATION (Warn)

Every `GuardResult<T>` request-guard binding must be propagated with `?` or
otherwise handled.

Detection (compiler-resolved, not textual):

- A handler parameter counts iff its declared type resolves (HIR) to the
  `router::GuardResult` type alias **and** the body's typeck results say the
  binding's type is `core::result::Result` (diagnostic item). A same-spelled
  lookalike alias, or an alias over a non-`Result` type, cannot match.
- Uses are classified by parent node: `let _ = x;` / `drop(x)` = discard;
  `let b = x;` (binding pat, direct init) = neutral — this covers the
  compiler-generated `let param = param;` that moves `async` fn params into
  the future, plus user aliasing, followed transitively as further names for
  the value; anything else (`?`, `match`, field access, call args) =
  propagation. Fail closed: flag unless at least one use propagates
  (zero uses also flags — an unenforced guard is a violation).

`?` needs no special case: it desugars to `match branch(x) { ... }`, so the
binding use sits under a call, never directly under a `let`.

### AUTH3_NO_FORWARD_DENIAL (Warn)

No `Outcome::Forward` in a `FromRequest` guard impl (AUTH-3: denial is an
error, not a fall-through). A guard that rejects must return
`Outcome::Error` with the declared status; `Outcome::Forward` only selects
the next ranked route, never denies.

Detection (compiler-resolved, not textual):

- The enclosing impl counts iff its trait ref resolves (HIR `Res::Def`) to
  a `FromRequest` trait under a `request` module (currently
  `rocket::request::from_request::FromRequest`). A same-named user trait or
  an inherent impl cannot match.
- Inside such an impl, an `Outcome::Forward` path matches iff its `qself`
  resolves to the `Outcome` enum (`QPath::TypeRelative` — the shape
  hand-written `Outcome::Forward(..)` takes) or it resolves via its `Ctor`
  def to an `Outcome` variant — or, fail closed, its spelling ends in
  `Outcome::Forward` even when resolution is unavailable.
- Rocket-generated route shims also mention `Outcome::Forward`, but those
  live outside any `FromRequest` impl and never match (verified: ~116
  generated mentions across the tree, zero findings).

Triage of the 3 existing sites (all `backend/src/router/auth.rs`) — each is
a genuine denial-as-forward, none is legitimate route-variant selection, so
no allowlist exception is recorded and the lint warns on the clean tree:

- `GuardHashOriginal::from_request` (~line 929): a valid token without
  `allow_original` returns `Forward(Unauthorized)`. The route consumes the
  guard via `GuardResult<..>`, and Rocket's `Result<T, E>` guard impl
  propagates `Forward` (`Forward(status) => Forward(status)`), so the
  denied request falls through to lower-ranked routes — including the
  rank-11 `spa_fallback` GET catch-all, which serves the SPA shell (200)
  instead of the declared 401. Violation.
- `TimestampGuardModified::from_request` (~lines 1064, 1068): a missing or
  undecodable bearer token returns `Forward(Unauthorized)`. The guard is
  consumed directly by `POST /post/renew-hash-token` (no ranked alternative
  route), so denial surfaces as 404 instead of the documented 401.
  Violation (×2 sites, one cause).

If a future site is judged legitimate route-variant selection, record it
with a per-site `#[allow(auth3_no_forward_denial)]` plus the reason — no
project-wide suppression lives in the lint.

Known limits (both lints):

## Running

Install the driver once (pinned version, idempotent):

```sh
just install-dylint
```

Run the checks (advisory — not part of `just check`):

```sh
just dylint-check
```

Under the hood this is `cargo dylint --all -- -p picasu`: cargo-dylint
finds this crate through `[workspace.metadata.dylint]` in the root
`Cargo.toml`, builds it with its pinned nightly (see Toolchain below),
then runs `cargo check -p picasu` under that same nightly with the lint
registered. Build products land in the workspace `target/dylint/`
(covered by the root `/target/` ignore), never in this crate's directory.

## Adding a lint

1. Add a `declare_lint!` + `declare_lint_pass!` pair, a
   `LateLintPass` impl, and a `register_late_lint_pass` line in the shared
   `register_lints` in `src/lib.rs`, following `AUTH3_NO_FORWARD_DENIAL`.
   (A second `declare_late_lint!` does not work: each expansion emits its
   own `dylint_version`/`register_lints`, so multi-lint crates register
   manually — see `dylint_linting` docs, "A library containing more than
   one lint".)
2. Verify both directions through `just dylint-check`: clean tree first
   (expect zero findings), then apply a one-line violation, re-run (exactly
   that site must warn), then `git checkout --` the file and confirm
   `git status` is clean — never leave the violation in the tree.
3. Document the lint in this file (what it flags, known limits) and, if it
   moves a rule out of `openapi-sanity`, update the disposition in
   `.plan/api-assurance.md`.

## Known limits

- A parameter spelled directly as `Result<T, AppError>` (bypassing the
  alias) is not tracked — the discipline mandates the `GuardResult` spelling.
- `if let Ok(..) = guard` without a diverging else is not yet detected
  (match-scrutinee analysis is open work).
- `#[allow(auth2_guard_propagation)]` / `#[deny(...)]` work as with any lint.
- AUTH-3: an `Outcome` imported under a different name still matches by
  resolution; only a fully unresolvable spelling falls back to the
  `Outcome::Forward` name check. A `Forward` constructed outside a
  `FromRequest` impl (e.g. Rocket-generated route shims) never matches.
- `#[allow(auth3_no_forward_denial)]` / `#[deny(...)]` work as with any
  lint; use per-site `allow` with a recorded reason for any future
  route-variant exception.

## Toolchain

Pinned via `rust-toolchain.toml` to `nightly-2026-08-01` **for this crate
only**. The main build stays on stable; this crate is deliberately _not_ a
workspace member so stable `cargo check --workspace` never touches
`rustc_private` code. Requires the `rustc-dev` component:

```sh
rustup component add --toolchain nightly-2026-08-01 rustc-dev llvm-tools
```

Built against `dylint_linting 6.1.0`, driven by `cargo-dylint 6.1.0` (same
version, installed via `just install-dylint`). Re-pin quarterly,
driven by `dylint_linting` releases: bump the pin here and the
`dylint_version` justfile var together, since the check toolchain
must match the toolchain the lint library builds with.
