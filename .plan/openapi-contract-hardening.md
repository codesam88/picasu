---
status: open
type: feature
priority: high
area: backend
---

## Notes

Audit of `docs/openapi-reference.md` (widdershins output) against
`backend/openapi.json`, the utoipa annotations, and the actual Rocket routes.
Route coverage itself is complete (apparent gaps were parser artifacts of
multi-line attributes and Rocket `{x..}` segments); the problems are in the
contract content, organization, and rendering. No work started yet.

## Problem and Hardening Strategy

The exposed API can drift even when a focused rework follows the intended
path-primary design and removes known migration artifacts. The recent review
found stale naming, incorrect identifier values at live call sites, missing
OpenAPI operations, incomplete authentication responses, and documentation
that rendered incorrectly. These defects were not caught by the existing API
or Playwright scenarios because those tests cover selected workflows rather
than the complete public contract. The generated OpenAPI document also did
not prevent drift: route registration, annotations, generated paths, and
public-spec filtering are separate sources of truth.

The first five hardening mechanisms should be established as recurring CI
checks and treated as a single API review gate:

1. **Checked-in generated public spec.** Generate the normalized public
   `openapi.json` in CI and compare it with the reviewed repository artifact.
   Any route, parameter, schema, response, security, or documentation change
   must appear in the diff and receive normal code review.
2. **Mounted-route/spec parity.** Compare the actual mounted `(method, path)`
   routes with the operations in the public spec. Fail on undocumented routes,
   stale spec operations, duplicate operation IDs, and accidental exposure of
   test-only or internal routes. Prefer an explicit route inventory or runtime
   route metadata over regex-only source discovery.
3. **OpenAPI structural linting.** Enforce project rules for operation IDs,
   tags, summaries, descriptions, request schemas, success/error responses,
   security requirements, path/query parameters, and named schemas. This
   should catch incomplete annotations even when an operation is present.
4. **Breaking-change detection.** Diff the generated spec against the latest
   released baseline and classify removed operations, narrowed schemas,
   newly-required fields, enum changes, response changes, and security changes
   as breaking or review-required. Require an explicit override or release
   note for accepted breaking changes.
5. **Spec-driven contract smoke tests.** Use the OpenAPI document to exercise
   every operation at least for reachability, authentication behavior, input
   validation, unknown-field rejection, expected status families, and response
   schema validation. Start with deterministic seeded fixtures and expand to
   property-based testing only where the endpoint state model permits it.

## Tasks

High — contract wrong or materially incomplete:

- [ ] Register `POST /post/renew-hash-token` in `openapi.rs` — it has an
      utoipa annotation (`auth.rs`) but was never added to the paths list, so
      it is absent from the spec; sibling `/post/renew-timestamp-token` is
      documented (asymmetric omission). Add a coverage test comparing mounted
      routes against the spec so this class of omission cannot recur.
- [x] Document `401` on guarded operations: only 2/65 ops currently declare
      `401` (`authenticate`, `/unauthorized`) while data endpoints sit behind
      `GuardAuth`/`GuardTimestamp` (24 router files). Introduce a reusable
      `Unauthorized` response component and sweep it across `#[utoipa::path]`
      responses (or post-process the generated spec). **Done** — one
      `Unauthorized` `ToResponse` component in `backend/src/openapi_components.rs`,
      registered by the `build.rs` generator from a single `RESPONSE_COMPONENTS`
      list, referenced by 39 operations (2 → 40 declaring a 401: +37 in the
      sweep, +2 when `get-rows`/`get-scroll-bar` were fixed). Each route was
      checked individually: `GuardReadOnlyMode` answers 405;
      `get-rows`/`get-scroll-bar` originally discarded their guard result —
      that gap was fixed (guard propagated, both operations now declare 401)
      and recorded in `bug-get-rows-auth-guard-discarded.md` (done).
      `POST /post/authenticate` lost its specific "Invalid
      password" wording because utoipa cannot attach a description to a `$ref`;
      the shared description covers it. Follow-up: 405 is declared on only one of
      the 20 `GuardReadOnlyMode` routes.
- [x] Remove test-only probes from the public spec: `probe_record`,
      `probe_dupe_group` + `TestRecordProbe`/`DupeGroupMember` schemas are
      registered unconditionally; handlers compile into production and are only
      flag-gated (404 unless test bootstrap). **Done** — `--dump-openapi` now
      serves `openapi_public::public_json()`, which strips `/get/test/*` and
      the two probe schemas; probe contract tests keep the full generated spec.

Medium — bad patterns and type fidelity:

- [ ] Tag every operation: 39/65 ops have no tags, the other 26 all carry the
      misnomer `pages`; reference grouping/TOC is effectively random. Suggest
      `auth`, `albums`, `assets`, `config`, `index`, `metadata`, `shares`,
      `serving`, `upload`.
- [ ] Add missing schema field descriptions: ~204 `none` cells across ~30 of
      39 schemas (`EditTagsData`, `DeleteList`, `CreateShare`, `Prefetch`, …).
      Existing hand-written `///` comments (`FileEntry`, `coverHash`) are the
      quality bar.
- [ ] Fix nullable/union rendering: `Option<T>` collapses to `any` in tables
      and `{}` in examples (`TestRecordProbe.path`,
      `PrefetchReturn.resolvedShareOpt`, multipart upload `body`). Emit
      explicit nullable schemas so generators keep the type.
- [ ] Fix `FsCompletion` wire casing: serializes `is_default` (snake) while
      every other schema is camelCase — missing
      `#[serde(rename_all = "camelCase")]`. Real API inconsistency, not just
      docs.
- [ ] Fix mangled anchors: 4 operations emit `<h3 id>` attributes containing
      raw newlines/backticks/apostrophes from multi-line utoipa summaries
      (album-index, index-image, probe ops), breaking Parameters/Responses
      in-page links and duplicating description-as-anchor. Keep utoipa
      summaries single-line titles.
- [x] Update `docs/openapi-generator.md`: 4 references to
      `docs/mdbook/src/openapi-reference.md` are stale; `justfile` writes
      `docs/openapi-reference.md`. Anything wiring a CI drift-check against
      the doc path checks a nonexistent file. **Done** — paths corrected, the
      nonexistent `just openapi-docs`/`openapi-docs-check` recipes replaced with
      the real `just docs-openapi`/`just openapi-check`, and the checked-in
      artifact plus the parity gate documented.

Low — consistency and polish:

- [ ] Document remaining `/upload` query params on the utoipa path: only `auto_rename` is annotated; add
      `presigned_album_id_opt` and `on_conflict` (valid values `skip`|`rename`, defaults). Absorbed from
      `openapi-upload-query-params.md` (2026-09-24).
- [ ] Add descriptions to bare enums: `OnConflict` (`skip`/`rename` need
      behavior semantics), `AlbumIndexState`.
- [ ] Decide a naming convention and document it: snake query params
      (`on_conflict`, `auto_rename`) vs camel bodies (`onConflict`) vs mixed
      path styles with verb-doubling (`/get/get-data`). Path changes are
      breaking — documenting the rule is enough for now.
- [ ] Decide static-route policy: `/assets/<file..>` is undocumented while
      the SPA catch-all `/{path}` is documented; pick neither or both.
- [ ] Rename `DataBaseTimestampReturn` (typo-cased legacy name).
- [ ] Optional bloat pass: 65 ops × 9 sample languages ≈ 70% of the 11k-line
      file, including 39 widdershins "backwards compatibility" boilerplate
      hits; consider `widdershins --summary` or trimming sample languages.

## Progress

- 2026-09-25: Documented the 401 contract. The sweep exposed a real security
  gap rather than only a documentation one: `GET /get/get-rows` and
  `GET /get/get-scroll-bar` discarded `GuardResult<GuardTimestamp>` with
  `let _ = auth;`, so both answered 200 to an unauthenticated caller, and
  `/get/get-scroll-bar` panics on an unknown snapshot id without credentials.
  Recorded as `bug-get-rows-auth-guard-discarded.md` and fixed separately, since
  it is a behavior change rather than documentation: both handlers now
  propagate the guard, both operations declare 401, and two scenarios assert
  401 for missing, malformed and expired tokens. The contract
  tests for 401 are bidirectional: an operation declaring a 401 must be listed in
  `GUARDED_OPERATIONS`, and a listed operation that stops existing fails with a
  stale-entry message instead of a missing-declaration one.
- 2026-09-25: Added negative self-checks so the gates cannot be neutered
  silently. The `routes![]` scanner moved out of `build.rs` into
  `backend/build/route_scan.rs`, shared with `src/tests/route_scan.rs`, because
  a build script cannot be unit-tested in place — its parsing bug is exactly
  what made the renewal routes undocumented. The parity comparison moved into
  pure functions (`undocumented_routes`, `stale_operations`,
  `duplicate_operation_ids`) that `src/tests/openapi_contract.rs` exercises with
  deliberately drifted inputs, and a new test asserts `public_json()` equals the
  committed `backend/openapi.json`, so artifact drift fails `cargo test` and not
  only `just openapi-check`. 13 mutations (neuter each comparison, drop the
  exclusion filter, widen the exclusion, ignore the HTTP method, no-op the path
  normalizer, revert the scanner to line-based parsing, accept non-identifier
  entries, stop stripping comments, end the block at the first bracket, scan
  only the first block, stale artifact) were each verified to fail a named test.
- 2026-09-25: Established hardening mechanisms 1 and 2.
  `backend/openapi.json` is now a committed, pretty-printed, sorted-key
  artifact; `just openapi-check` regenerates and diffs it and is part of
  `just check` (so CI and the `main` pre-commit hook run it).
  `backend/src/tests/openapi_contract.rs` compares Rocket's mounted route table
  with the spec: undocumented mounted routes, documented-but-unmounted
  operations, duplicate `operationId`s, and dead contract exclusions all fail.
  Rocket's `<segment>`/`<segment..>` URI syntax is normalized to OpenAPI
  `{segment}` templates; the test-only probes and the `/assets` file server are
  explicit, reviewed exclusions. Both directions were verified by injecting
  drift: renaming an annotated path fails `every_spec_operation_is_mounted`,
  and un-scanning `router/auth.rs` fails `every_mounted_route_is_documented`.
  Spec operation count went 59 → 61. Remaining: structural linting (3),
  breaking-change detection (4), spec-driven contract smoke tests (5), plus the
  content tasks above. The markdown reference is still not drift-checked
  because `widdershins` is fetched over the network.
- 2026-09-23: Rework-adjacent subset executed by the path-primary cleanup
  sweep (see `.plan/path-primary-cleanup.md`): test-only probes stripped from
  the public spec; `PUT /put/assign_album` given tag `albums`, a single-line
  summary and explicit description (fixes its mangled reference anchors);
  `OnConflict` and `AssignAlbumData.albumId` documented. Reference regenerated.
- 2026-09-23: Dependency for task 1 (`renew-hash-token`): decide
  `path-primary-cleanup` category 7/B3 first — if the route is renamed
  hash→asset token, register the final path instead of documenting the old one
  and then changing it.
- 2026-09-24: Superseding the B3 dependency note: `path-primary-cleanup`
  category 7/B3 was withdrawn (content hash intentionally stays in compressed
  URLs and token claims; route remains `/post/renew-hash-token`). Task 1's
  rename dependency is resolved — it may proceed at any time via the
  `build.rs` `router/auth.rs` route-scan fix.
