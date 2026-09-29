---
status: in-progress
type: feature
priority: high
area: backend
---

## Notes

Reduce the OpenAPI gate to two backend-side pieces and delete the
`openapi-sanity` crate: **generation** (the build script that derives
`openapi.rs` from `routes![]` and `#[utoipa::path]`) and **`--check-openapi`**
(the runtime comparison of a real build's mount table against the committed
document). Decided 2026-09-28 on branch `openapi-routes-gate`, cut from
`origin/main`.

What is given up, stated plainly: the source/spec rule set (route coverage
diagnostics, tag policy, the auth-policy table and the guard checks including
the discarded-`GuardResult` class). Nothing else in the repository covered
those, so the `84f29aa5` `get_rows` shape is no longer checked. What is kept is
the half that no external tool can do: a route mounted without documentation,
or documented without a route, fails.

**Work items, in order.** Each lands as its own reviewed commit.

1. **Port `to_spec_path` into the backend** (`backend/src/spec_path.rs`), with
   the tests from the analyzer's `tests/paths.rs` and the backend's duplicated
   cases. It is the mount-table↔document translation; the backend is its only
   consumer.
2. **Gate the `/get/test/` probe registration behind `#[cfg(test)]`**
   (`router/get/mod.rs` plus the stale comments that say the probes mount in
   every build). Required by item 3: the public document never carried the
   probes, so a shipped build that mounts them would fail the gate. Observable
   only from an integration test, since in-crate tests run with `cfg(test)`.
3. **Add `--check-openapi`** (`backend/src/openapi_parity.rs`): read the
   committed document from disk, build the real `build_rocket()`, compare under
   the asymmetric rule (mounted ⇒ documented; documented-but-unmounted is drift
   unless feature-gated with the feature off), drop
   `CONTRACT_EXCLUSION_PREFIXES`, exit non-zero with a per-route report. Fixture
   tests for the rule in both directions, plus the pins that every
   `x-picasu-feature` value is a declared feature and that
   `CONTRACT_EXCLUSION_PREFIXES` still names `/assets`. Wire as a phase of
   `just openapi-check`, into the pre-commit hook through it, and onto the
   binary the release job ships.
4. **Move the source scanning into the backend.** `build.rs` currently calls
   `openapi_sanity::{scan_routes, scan_handlers, Finding, SCANNED_MODULES}`.
   Move the minimum the generator needs (`routes.rs`, `handlers.rs`, the
   `Finding` type, the module-path resolution and the file list) under
   `backend/build/` as build-script modules, and drop the build-dependency.
   Acceptance is strict: `just openapi-gen` must leave `backend/openapi.json`
   byte-identical.
5. **Delete `utils/openapi-sanity`** and everything wired to it: the workspace
   member, the `openapi-sanity`/`openapi-check`/`utils-check`/`utils-test`/
   `utils-format` recipe references, the backend's dev-dependency,
   `backend/src/tests/route_scan.rs` (its build-script text assertion dies with
   the crate it named; the module list's existence is now a build error), the
   auth-policy and tag-policy tests in `openapi_contract.rs` (the route-set
   parity tests stay, as the self-checks they are), `docs/openapi-generator.md`
   rewritten around generation and the gate, and the superseded plan files
   removed. A `grep` for `openapi_sanity` outside the build script must come
   back empty.
6. **Verify.** `just check`, `just test` (the eight pre-existing Playwright
   failures are known and unrelated), the artifact diff from item 4, and a
   deliberate drift proving the gate fails.

**Not in scope here:** the document linter question (an external OpenAPI
linter for the document-only rules), and `rocket_extras` (utoipa's optional
feature that derives request bodies and query parameters from the Rocket
attribute — it would reduce what can drift but reports none of it). Both were
raised on `openapi-params-gate`; neither is needed to land this branch.
