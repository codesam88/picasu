---
status: open
type: feature
priority: high
area: backend
---

## Goal

Create a dedicated `openapi-sanity` analyzer for source-to-public-contract
consistency. It owns facts that are currently split between `build.rs`,
`openapi_contract.rs`, and duplicated test helpers:

- which handlers and routes exist in source;
- which OpenAPI annotations belong to each handler;
- whether Rocket and OpenAPI paths/methods agree;
- whether documented auth policy matches the actual handler guard shape.

It is not a runtime behavior test, a generated-artifact diff, or a general Rust
taint analyzer. Those remain separate mechanisms. `just openapi-check` is the
single public-contract command and will run both the semantic sanity checks and
the generated-artifact diff.

## Current Ownership

The plan must preserve these existing responsibilities rather than duplicate
them:

| Responsibility                      | Existing owner                          | Future owner                             |
| ----------------------------------- | --------------------------------------- | ---------------------------------------- |
| Generate `openapi.rs`               | `backend/build.rs`                      | `build.rs` using the shared library      |
| Generated artifact freshness        | `just openapi-check`                    | one subcheck of `openapi-check`          |
| Runtime mounted-route parity        | `backend/src/tests/openapi_contract.rs` | backend integration test                 |
| Endpoint auth behavior              | API scenarios/tests                     | unchanged                                |
| Source route/annotation consistency | build warnings + AST tests              | `openapi-sanity`                         |
| Tag and 401 policy                  | OpenAPI contract tests                  | migrate only after equivalent CLI checks |
| Request-to-panic candidates         | `build/reachability.rs` registry        | separate reachability work               |
| Markdown API generation             | `docs-openapi`                          | unchanged                                |

The current generator already provides useful sanity checks: annotation coverage
and generated `__path_*` registration. The new crate should make those checks
reusable and more actionable, not reimplement OpenAPI serialization.

## Architecture

Add a workspace crate at `utils/openapi-sanity/`:

- `lib.rs`: source model, OpenAPI model, policies and diagnostics;
- `src/main.rs`: deterministic CLI for CI and local use;
- unit fixtures for malformed source, route macros, annotations and policies.

`backend/build.rs` consumes the library for the route list needed to generate
`openapi.rs`. Backend tests consume the same library where source/spec checks
are useful. The CLI consumes router source plus the checked-in
`backend/openapi.json`.

Do not keep a second parser in `backend/build` once extraction is complete.
Runtime route inspection remains in backend tests because source parsing cannot
prove which routes Rocket actually mounts under every feature configuration.

## Detailed Implementation Steps

### Step 1: Extract the shared source analyzer

- Move `backend/build/ast_scan.rs` and `backend/build/route_path.rs` into the
  new crate without changing behavior.
- Preserve route macro parsing, per-function annotation detection, path
  normalization, malformed-source findings and negative tests.
- Make the result model explicit: handler name, method, Rocket URI, normalized
  OpenAPI path, annotation presence, annotation path, operation ID and source
  location.
- Make the analyzer return diagnostics instead of printing or touching files.
- Make `build.rs` consume the library and retain only filesystem/build-output
  responsibilities.
- Remove the `#[path]` duplication after all consumers use the crate.

Acceptance:

- `backend/src/openapi.rs` is byte-identical before and after;
- `backend/openapi.json` is unchanged;
- existing AST/parity tests pass;
- `cargo build --locked` passes;
- malformed source produces a diagnostic, never a panic.

### Step 2: Add source/spec CLI validation

- Add the analyzer as a deterministic CLI command, but invoke it through
  `just openapi-check`; contributors should not need to remember a second
  public-contract command.
- Read source through the shared analyzer and parse the public OpenAPI artifact.
- Report stable file/line diagnostics for:
  - missing function-local annotations;
  - Rocket/OpenAPI path disagreement;
  - method disagreement;
  - source operations absent from the spec;
  - spec operations without a source declaration;
  - duplicate handler identities and duplicate operation IDs.
- Make `just openapi-check` run the semantic analyzer and then the generated
  artifact diff, with nonzero status on either failure.
- Keep `just check` and CI wired to `openapi-check`; do not add a second
  independently required contract command.

Acceptance:

- Injected missing annotation fails;
- injected path or method mismatch fails;
- injected stale/source-only operation fails;
- injected duplicate operation ID fails;
- reverting each mutation returns the command to green.

### Step 3: Add actual auth-policy consistency

The current `GUARDED_OPERATIONS` list checks OpenAPI `401` declarations but does
not verify that the corresponding handler is actually guarded. Replace that gap
with source-aware facts:

- derive observed guard types from handler parameters;
- distinguish direct guards such as `GuardAuth` from deferred
  `GuardResult<GuardAuth>`;
- report deferred guard results that are discarded rather than propagated or
  inspected;
- map observed guard classes to an explicit auth policy;
- require the OpenAPI operation to declare the matching security/401 policy;
- require protected operations to have an observed approved guard;
- require public operations to be explicit exceptions, not accidental omissions.

Do not infer authentication from subject tags such as `albums` or `timeline`.
Tags are presentation metadata, not security policy.

Initially, keep auth policy exceptions in a small declarative policy file. Do
not duplicate every route manually if the handler signature already provides
the fact. The policy should cover entry points such as authentication,
timestamp-token reads, share routes, uploads and public pages.

Acceptance:

- Removing a guard from a protected handler fails;
- adding a guard without documenting the corresponding policy fails;
- discarding `GuardResult` fails;
- direct guards that are enforced by Rocket do not become false positives;
- public-page and intentional unauthenticated exceptions are explicit.

### Step 4: Migrate existing tag and response policy

Only after Steps 1–3 are green:

- move the current known-tag policy into the shared policy library;
- move the shared `401 Unauthorized` reference policy into the CLI;
- retain backend runtime tests for mounted-route parity and endpoint behavior;
- run old and new checks together and compare diagnostics;
- delete duplicated checks only after equivalent output is demonstrated.

Do not expand this step into general schema linting or every possible HTTP error
family. The `405 GuardReadOnlyMode` task remains a separate, explicit contract
decision.

Acceptance:

- operation counts and exclusions match current tests;
- all current negative self-checks remain meaningful;
- no generated artifact changes without an intentional annotation change.

### Step 5: Parameter and documentation consistency

Consider as a follow-up after the core gate:

- OpenAPI path parameters match handler parameters;
- optional query parameters match `Option<T>` shapes;
- request body annotations match `Json<T>`/upload inputs;
- operation IDs are stable across reviewed spec changes;
- response schemas are referenced and not orphaned.

These checks should be added only where the analyzer has enough source
information to produce actionable diagnostics. Do not build a general schema
reimplementation in `openapi-sanity`.

### Step 6: Documentation and maintenance

- Document the crate and the semantic phase of `just openapi-check` in
  `docs/openapi-generator.md`.
- Document the distinction between source checks, generated-artifact checks,
  runtime parity and endpoint behavior tests.
- Record unsupported syntax and feature/cfg assumptions in crate docs.
- Add mutation tests for every policy rule that can silently become a no-op.
- Consider a scheduled external cross-check only if it supplies independent
  value; it is not a required PR gate.

## Non-Goals

- Do not replace runtime endpoint tests with source analysis.
- Do not make `openapi-sanity` own OpenAPI serialization or Markdown generation.
- Do not put the request-to-panic registry in this crate; it belongs to the
  separate reachability analyzer.
- Do not treat tags as authentication policy.
- Do not depend on CodeQL attribute-token extraction; current CodeQL Rust
  extraction does not expose the macro arguments required for this contract.
- Do not change endpoint behavior as part of analyzer extraction.
- Do not make network-fetched `widdershins` generation a CI requirement.

## Success Criteria

- One shared implementation owns source route discovery and Rocket-to-OpenAPI
  path normalization.
- `build.rs`, backend tests and the CLI consume the same library.
- `just openapi-check` is the single semantic and generated-artifact contract
  gate.
- `just check` and CI run that gate without requiring a second contract command.
- Runtime parity and endpoint behavior tests remain separate and meaningful.
- A documented guarded route cannot drift into an unguarded handler or vice
  versa without a failing diagnostic.
- Existing route behavior, OpenAPI output and intentional exclusions remain
  unchanged.
