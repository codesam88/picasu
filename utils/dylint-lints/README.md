# dylint-lints

Project-specific Rust lints for the authorization guard discipline, enforced
with full compiler type information via Dylint. They cover what text-level
analysis cannot: resolved types, cross-item guard lookup, desugared control
flow. Rules that need none of that stay in `utils/openapi-sanity`; the
disposition per rule is recorded in `.plan/api-assurance.md`.

## Lint catalog

| Lint                             | Folder                                                 | One-line                                                                                                                                                                                                               |
| -------------------------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `AUTH2_GUARD_PROPAGATION` (Warn) | [`auth2_guard_propagation/`](auth2_guard_propagation/) | Every `GuardResult<T>` request-guard binding must be propagated with `?` or otherwise handled.                                                                                                                         |
| `AUTH3_NO_FORWARD_DENIAL` (Warn) | [`auth3_no_forward_denial/`](auth3_no_forward_denial/) | No `Outcome::Forward` in a `FromRequest` guard impl — denial is `Outcome::Error`, not a fall-through.                                                                                                                  |
| `DECODE_AGREEMENT` (Warn)        | [`decode_agreement/`](decode_agreement/)               | Each `FromRequest` guard decodes exactly its model-assigned claims type (`GuardAuth`/`GuardUser`→`Claims`, `GuardHash`/`GuardHashOriginal`→`ClaimsHash`, `TimestampGuardModified`/`GuardTimestamp`→`ClaimsTimestamp`). |

Each folder is a standalone lint crate (mirroring the upstream
`examples/<category>/<lint-name>/` layout: one directory per lint, each
with its own crate and README) and documents its lint fully: what it
catches, why it matters, a violating/fixed example pair, and its known
limits. Start from the folder README, not from here.

## Running

Install cargo-dylint once (pinned version, idempotent):

```sh
just install-dylint
```

Run the checks (advisory — not part of `just check`):

```sh
just dylint-check
```

Under the hood this is `cargo dylint --all -- -p picasu`: cargo-dylint
finds the lint crates through the glob entry in
`[workspace.metadata.dylint]` in the root `Cargo.toml`
(`utils/dylint-lints/*`), builds each with the pinned nightly (see
Toolchain below), then runs `cargo check -p picasu` under that same
nightly with the lints registered. Build products land in the workspace
`target/dylint/` (covered by the root `/target/` ignore), never in the
lint crate directories.

## Adding a lint

1. Scaffold a new folder `utils/dylint-lints/<lint_name>/` with its own
   `Cargo.toml` (`crate-type = ["cdylib"]`, `dylint_linting = "<pinned>"`,
   detached `[workspace]` so stable `cargo check --workspace` never touches
   `rustc_private` code), `src/lib.rs` (`dylint_linting::dylint_library!()`
   - `declare_lint!` + `declare_lint_pass!` + `register_lints`), and
     `README.md` following either existing folder. No change to the root
     `Cargo.toml` is needed: the `utils/dylint-lints/*` glob discovers the
     new crate automatically (each crate registers exactly one lint, so the
     old multi-lint manual-registration workaround does not apply).
2. Verify both directions through `just dylint-check`: clean tree first
   (expect zero findings), then apply a one-line violation, re-run (exactly
   that site must warn), then `git checkout --` the file and confirm
   `git status` is clean — never leave the violation in the tree.
3. Document the lint in its folder README (what it flags, why it matters
   with the AUTH property and failure mode, violating/fixed example pair,
   known limits) and, if it moves a rule out of `openapi-sanity`, update
   the disposition in `.plan/api-assurance.md`.

## Known limits

Per-lint limits live with their lints: see
[`auth2_guard_propagation/README.md`](auth2_guard_propagation/README.md#known-limits)
and
[`auth3_no_forward_denial/README.md`](auth3_no_forward_denial/README.md#known-limits).
The 3 known `AUTH3_NO_FORWARD_DENIAL` findings on the clean tree (all
`backend/src/router/auth.rs`, triaged as genuine denial-as-forward) are
documented in the
[AUTH-3 README](auth3_no_forward_denial/README.md#why-it-matters).

## Toolchain

Pinned via `rust-toolchain.toml` (one file at this directory's top, shared
by all lint crates below it) to `nightly-2026-08-01` **for these crates
only**. The main build stays on stable; the lint crates are deliberately
_not_ workspace members so stable `cargo check --workspace` never touches
`rustc_private` code. Requires the `rustc-dev` component:

```sh
rustup component add --toolchain nightly-2026-08-01 rustc-dev llvm-tools
```

Built against `dylint_linting 6.1.0`, driven by `cargo-dylint 6.1.0` (same
version, installed via `just install-dylint`). Re-pin quarterly,
driven by `dylint_linting` releases: bump the pin here and the
`dylint_version` justfile var together, since the check toolchain
must match the toolchain the lint libraries build with.
