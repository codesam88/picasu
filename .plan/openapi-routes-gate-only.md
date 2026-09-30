---
status: done
type: feature
priority: high
area: backend
---

## Progress

**Item 6 — verification run, 2026-09-30.** `just check` exits 0 (backend fmt +
clippy, utils fmt + clippy, frontend prettier/vue-tsc/eslint, docs-check,
plan-lint, `openapi-artifact` reporting "openapi.json matches the generated
spec", and `openapi-routes` reporting "61 routes and 61 documented operations
agree"). `just test` exits 0: 324 backend unit tests, 1/1/1/1/1 integration
tests, 35 `openapi-scan` and 24 `snapfab` tests, 74 vitest tests, and 36
Playwright scenarios. `cargo fmt --check`, `cargo clippy -- -D warnings -A
clippy::unwrap_used`, `just plan-lint` and `just docs-check` are clean.
Drift proof: a copy of `backend/openapi.json` with `paths./get/get-tags` deleted
makes `cargo run --package picasu -- --check-openapi <copy>` exit 1 and report
`1 mounted route missing from the spec: GET /get/get-tags`; the unmodified
document exits 0.

One number in item 6 did not reproduce: it expected eight pre-existing
`interpreter.spec.ts` failures, and this run had none — 36 of 36 passed. No new
failures appeared either, so nothing regressed, but the figure in the item text
should not be relied on; it is presumably environment- or run-dependent, and it
was not reproduced against a baseline of the same commit.

**Item 5 — done, 2026-09-30.** `utils/openapi-sanity` is deleted: the crate
(43 files, four fixture trees), the workspace member, the backend dev-dependency,
`src/tests/route_scan.rs` (the whole file — its `build.rs` text assertion named
the deleted crate, and `openapi-scan`'s own `tests/regressions.rs` covers what
was left of it), the `openapi-sanity` recipe and its phase of `just
openapi-check`, the `openapi-sanity` plan (the reduced-scope sibling of that
plan lives on `openapi-params-gate`, not here), and the crate's references in
`docs/openapi-generator.md` — rewritten around generation and the route-set gate
— and `docs/test-strategy.md`.

Coverage that went with it, stated so it is not rediscovered: the tag taxonomy
rules (no-tag, unknown-tag, `pages`-misplacement, in both directions) and the
whole auth policy — the `AUTH_POLICY` table, the guard observation (direct and
deferred, `GuardResult` discarded), and the guard/document agreement checks. The
`openapi_contract` tests that drove those were deleted rather than weakened:
`the_public_operations_follow_the_shared_tag_taxonomy`,
`self_check_detects_tag_drift_in_the_public_spec`,
`the_auth_policy_and_the_documented_unauthorized_responses_agree`,
`self_check_detects_a_protected_operation_without_a_documented_401`,
`self_check_detects_a_stale_auth_policy_entry`,
`self_check_detects_a_public_operation_that_documents_a_401`. The tag
vocabulary and guard table survive as documentation in
`docs/openapi-generator.md`, with nothing enforcing them. The
`Unauthorized`-component tests and the mounted-route parity self-checks stay.

`backend/openapi.json` is byte-identical after `just openapi-gen`.

**Item 4 — done, 2026-09-30.** The scanning went into a new workspace crate
`utils/openapi-scan` rather than into `backend/build/` modules as the item text
above says: build-script modules are invisible to `cargo test -p <crate>`, and
the ported test suites are the reason to keep a crate. `build.rs` imports
`openapi_scan::{scan_routes, scan_handlers, Finding, SCANNED_MODULES}`; the
`openapi-sanity` crate and its `just openapi-check` phase are untouched until
item 5.

`collect_all_routes` now **fails** on a `SCANNED_MODULES` entry it cannot read
instead of `continue`ing, which is what the deleted
`every_scanned_router_module_exists` used to catch. Verified by adding
`("fairing", "fairing/mod.rs")` back and building.

`backend/openapi.json` is byte-identical after `just openapi-gen`.

`Handler` was reduced to what `build.rs` reads — `name`, `line`, `annotated` —
so `HttpMethod` is gone and the scanner recognizes a route verb without
recording it. Two diagnostics that the four dropped fields used to carry were
kept as findings rather than as values, and one was added: a `#[utoipa::path]`
whose `path = ...` is not a string literal (or which carries no argument list)
is now reported, because the coverage count would otherwise credit a handler as
documented on a key that does not resolve. No real annotation in the router tree
trips it — the build log is clean.

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
