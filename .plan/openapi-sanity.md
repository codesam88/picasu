---
status: open
type: feature
priority: high
area: backend
---

## Goal

Create a dedicated `openapi-sanity` analyzer for the backend's public API
contract. It should become the single reusable implementation for source-level
route discovery, OpenAPI metadata validation, and policy checks, instead of
continuing to grow `build.rs` and duplicating `#[path]`-included analysis modules
inside the backend test target.

The analyzer is a contract checker, not a runtime behavior test and not a full
taint/reachability proof. Runtime route parity and endpoint scenarios remain in
the backend tests.

## Proposed Architecture

Add a workspace crate, preferably under `utils/openapi-sanity/`:

- Library API for parsing router source and checking a public OpenAPI document.
- CLI binary for `just openapi-sanity-check` and CI diagnostics.
- Unit fixtures for malformed source, route macros and policy violations.
- Shared route-path normalization, with no second implementation in tests or
  `build.rs`.

`backend/build.rs` consumes the library for the minimum route list needed to
generate `openapi.rs`. Backend integration tests consume the same library for
contract assertions. The CLI runs against the checked-in
`backend/openapi.json` and the router source tree.

## Checks

The first gate should cover:

1. Every Rocket route handler is discovered, including handlers registered in
   `routes![...]` blocks and direct mounts.
2. Every discovered public route has a function-local `#[utoipa::path]`.
3. Rocket URI and OpenAPI path agree after the canonical parameter/query
   normalization.
4. Every OpenAPI operation maps to a mounted/source route, with explicit
   exclusions for test probes and static assets.
5. Operation IDs are present and unique.
6. Every operation has exactly one known subject tag; `pages` is restricted to
   SPA page routes.
7. Auth policy is explicit: protected route classes require an approved guard
   type, and deferred `GuardResult` values must be propagated or inspected.
8. Required response policy is enforced, starting with the shared `401`
   `Unauthorized` component and the existing intentional landing-page
   exception.
9. Test-only routes and schemas are absent from the public artifact.
10. Registry entries for approved exceptions are valid, non-duplicated and
    non-stale.

## Iterative Implementation

### Step 1: Extract the analyzer

- Move the current `backend/build/ast_scan.rs` and `route_path.rs` logic into
  the crate library.
- Preserve byte-identical `backend/src/openapi.rs` and
  `backend/openapi.json` output.
- Preserve the current parser and negative tests.
- Remove the duplicated `#[path]` module arrangement after consumers use the
  crate.

Acceptance: `cargo test`, `just check`, `just openapi-check` and locked builds
pass with no generated artifact diff.

### Step 2: Add the CLI and source/spec checks

- Add a deterministic `openapi-sanity-check` command.
- Emit stable, file/line-oriented diagnostics and nonzero status on violations.
- Run it from `just check` and CI.
- Keep `openapi-check` as the generated-artifact diff; the new command validates
  semantics and policy.

Acceptance: injected missing annotation, path mismatch, duplicate operation ID,
unknown tag and stale registry entries each fail the command; reverting each
mutation returns it to green.

### Step 3: Move existing contract policy

- Move the current OpenAPI parity, tag, shared-401 and registry checks to the
  library/CLI where they are source-independent.
- Keep a small backend integration test for runtime Rocket route parity and
  endpoint behavior.
- Delete duplicated policy implementations only after the replacement has run
  successfully in CI-equivalent commands.

Acceptance: the CLI and backend tests report the same intentional exclusions and
the same operation counts.

### Step 4: Auth and response policy hardening

- Make the approved guard taxonomy data-driven rather than a hardcoded test
  list.
- Add explicit policy entries for public pages, share routes, upload and
  timestamp-token routes.
- Expand response checks to the remaining documented error families, including
  the `GuardReadOnlyMode` 405 gap.

Acceptance: adding a protected route without an approved guard or required error
response fails with an actionable policy diagnostic.

### Step 5: Documentation and maintenance

- Document the crate and command in `docs/openapi-generator.md`.
- Add a contributor workflow for changing routes and annotations.
- Record analyzer assumptions and unsupported syntax in the crate docs.
- Add a scheduled cross-check path for CodeQL/other external inventories only
  if it provides independent value; it is not a required PR gate.

## Non-Goals

- Do not replace runtime endpoint tests with source analysis.
- Do not treat `openapi-sanity` as a general Rust taint analyzer.
- Do not depend on CodeQL attribute-token extraction; current CodeQL Rust
  extraction does not expose the macro arguments needed for this contract.
- Do not change endpoint behavior as part of the analyzer extraction.
- Do not regenerate the Markdown API reference in CI while it depends on a
  network-fetched `widdershins` binary.

## Success Criteria

- One implementation owns route discovery and Rocket-to-OpenAPI path
  normalization.
- `build.rs`, backend tests and the CLI consume the same library.
- `just check` runs the semantic contract gate.
- Generated artifacts remain separately diff-gated by `just openapi-check`.
- Violations fail deterministically with actionable diagnostics.
- Existing route/API behavior and all current exclusions remain unchanged.
